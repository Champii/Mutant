use crate::error::Error;
use crate::index::{PadInfo, PadStatus};
use crate::internal_events::invoke_put_callback;
use crate::network::Network;
use crate::ops::{DATA_ENCODING_PRIVATE_DATA, DATA_ENCODING_PUBLIC_DATA, DATA_ENCODING_PUBLIC_INDEX};
use autonomi::ScratchpadAddress;
use log::info;
use mutant_protocol::{PutCallback, PutEvent, StorageMode};
use std::sync::Arc;
use tokio::sync::RwLock;

use super::context::Context;
use super::pipeline::write_pipeline;

/// Efficiently update a key with new content by reusing pads with matching checksums.
///
/// This function:
/// 1. Compares checksums between existing pads and new data chunks
/// 2. Reuses pads with matching checksums (keeps them as is)
/// 3. Marks pads with different checksums as needing update (sets to Free status)
/// 4. Handles cases where the new data is longer or shorter than the existing data
/// 5. Preserves the public index pad for public keys
///
/// This approach is more efficient than the original update method because it:
/// - Avoids unnecessary network operations for unchanged data chunks
/// - Preserves pad addresses when possible, reducing network churn
/// - Only uploads data that has actually changed
pub async fn update(
    index: Arc<RwLock<crate::index::master_index::MasterIndex>>,
    network: Arc<Network>,
    key_name: &str,
    content: Arc<Vec<u8>>,
    mode: StorageMode,
    public: bool,
    no_verify: bool,
    put_callback: Option<PutCallback>,
) -> Result<ScratchpadAddress, Error> {
    info!("Efficient update for {}", key_name);

    // Get existing pads for the key
    let existing_pads = index.read().await.get_pads(key_name);
    if existing_pads.is_empty() {
        return Err(Error::Internal(format!("Key '{}' not found", key_name)));
    }

    // Calculate chunk ranges for the new content
    let chunk_ranges = index.read().await.chunk_data(&content, mode.clone());

    // Special handling for public keys to preserve the index pad
    let preserved_index_pad = if public && index.read().await.is_public(key_name) {
        info!("Preserving public index pad for key {}", key_name);
        index.read().await.extract_public_index_pad(key_name)
    } else {
        None
    };

    // Compare the number of pads needed for the new content vs. existing pads
    let existing_data_pads_count = existing_pads.len();

    let new_data_pads_count = chunk_ranges.len();

    info!(
        "Update for key '{}': existing pads: {}, new chunks: {}",
        key_name, existing_data_pads_count, new_data_pads_count
    );

    // Create a vector to hold the updated pads
    let mut updated_pads = Vec::with_capacity(new_data_pads_count);

    // Process existing pads up to the minimum of existing and new counts
    let min_count = std::cmp::min(existing_data_pads_count, new_data_pads_count);

    for i in 0..min_count {
        let pad = &existing_pads[i];
        let chunk_range = &chunk_ranges[i];
        let chunk_data = &content[chunk_range.clone()];
        let chunk_checksum = PadInfo::checksum(chunk_data);

        // If checksums match, keep the pad as is
        if pad.checksum == chunk_checksum {
            info!("Pad {} (chunk {}) has matching checksum, keeping as is", pad.address, i);
            updated_pads.push(pad.clone());
        } else {
            // If checksums don't match, mark the pad for update
            info!("Pad {} (chunk {}) has different checksum, marking for update", pad.address, i);
            let mut updated_pad = pad.clone();
            updated_pad.status = PadStatus::Free;
            updated_pad.checksum = chunk_checksum;
            updated_pad.size = chunk_data.len();
            updated_pad.last_known_counter += 1;
            updated_pads.push(updated_pad);
        }
    }

    // If we need more pads than we have, acquire new ones
    if new_data_pads_count > existing_data_pads_count {
        info!(
            "Need {} additional pads for key '{}'",
            new_data_pads_count - existing_data_pads_count,
            key_name
        );

        // Acquire additional pads for the remaining chunks
        let additional_chunks = chunk_ranges[existing_data_pads_count..].to_vec();
        let additional_pads = index
            .write()
            .await
            .aquire_pads(&content, &additional_chunks)?;

        updated_pads.extend(additional_pads);
    } else if new_data_pads_count < existing_data_pads_count {
        // If we have more pads than we need, free the excess ones
        info!(
            "Freeing {} excess pads for key '{}'",
            existing_data_pads_count - new_data_pads_count,
            key_name
        );

        // Move excess pads to the free list
        let excess_pads = existing_pads[new_data_pads_count..].to_vec();

        index.write().await.import_raw_pads_private_key(excess_pads)?;
    }

    // Update the key in the master index
    if public {
        // For public keys, update with the preserved index pad
        if let Some(mut index_pad) = preserved_index_pad {
            index_pad.status = PadStatus::Free;
            index_pad.last_known_counter += 1;

            index.write().await.update_key_with_pads(key_name, updated_pads.clone(), Some(index_pad))?;
        } else {
            // This shouldn't happen for existing public keys, but handle it just in case
            index.write().await.update_key_with_pads(key_name, updated_pads.clone(), None)?;
        }
    } else {
        // For private keys, just update with the new pads
        index.write().await.update_key_with_pads(key_name, updated_pads.clone(), None)?;
    }

    // Get the address from the first pad
    let address = updated_pads[0].address;

    // Filter pads that need to be written (status is Free or Generated)
    let pads_to_write: Vec<PadInfo> = updated_pads
        .iter()
        .filter(|p| p.status == PadStatus::Free || p.status == PadStatus::Generated)
        .cloned()
        .collect();

    if !pads_to_write.is_empty() {
        info!(
            "Writing {} pads for key '{}' (out of {} total)",
            pads_to_write.len(),
            key_name,
            updated_pads.len()
        );

        let encoding = if public {
            DATA_ENCODING_PUBLIC_DATA
        } else {
            DATA_ENCODING_PRIVATE_DATA
        };

        let context = Context {
            index: index.clone(),
            network: network.clone(),
            name: Arc::new(key_name.to_string()),
            chunk_ranges: Arc::new(chunk_ranges),
            data: content.clone(),
            public,
            encoding,
        };

        // Write only the pads that need updating
        write_pipeline(context, pads_to_write, no_verify, put_callback.clone()).await?;
    } else {
        info!("No pads need to be written for key '{}'", key_name);
    }

    // For public keys, update and write the index pad
    if public {
        let (index_pad, index_data) = index.write().await.populate_index_pad(key_name)?;
        let index_data_bytes: Arc<Vec<u8>> = Arc::new(index_data);
        let index_chunk_ranges = Arc::new(vec![0..index_data_bytes.len()]);

        let index_pad_context = Context {
            index: index.clone(),
            network: network.clone(),
            name: Arc::new(key_name.to_string()),
            chunk_ranges: index_chunk_ranges,
            data: index_data_bytes,
            public,
            encoding: DATA_ENCODING_PUBLIC_INDEX,
        };

        // Write the index pad
        write_pipeline(
            index_pad_context,
            vec![index_pad],
            no_verify,
            put_callback.clone(),
        )
        .await?;
    }

    // Final completion callback
    invoke_put_callback(&put_callback, PutEvent::Complete)
        .await
        .unwrap();

    Ok(address)
}

pub async fn resume(
    index: Arc<RwLock<crate::index::master_index::MasterIndex>>,
    network: Arc<Network>,
    name: &str,
    data_bytes: Arc<Vec<u8>>,
    mode: StorageMode,
    public: bool,
    no_verify: bool,
    put_callback: Option<PutCallback>,
) -> Result<ScratchpadAddress, Error> {
    let pads = index.read().await.get_pads(name);

    if pads.iter().any(|p| p.size > mode.scratchpad_size()) {
        index.write().await.remove_key(name).unwrap();
        return first_store(
            index,
            network,
            name,
            data_bytes,
            mode,
            public,
            no_verify,
            put_callback,
        )
        .await;
    }

    let chunk_ranges = index.read().await.chunk_data(&data_bytes, mode.clone());

    // If the number of pads doesn't match the number of chunks, use the efficient update
    // which can handle adding or removing pads as needed
    if pads.len() != chunk_ranges.len() {
        info!(
            "Resuming key '{}' with data size mismatch. Index has {} pads, current data requires {}. Using efficient update.",
            name,
            pads.len(),
            chunk_ranges.len()
        );

        return update(
            index,
            network,
            name,
            data_bytes,
            mode,
            public,
            no_verify,
            put_callback,
        )
        .await;
    }

    // If all checksums match, we can just return success
    if index.read().await.verify_checksum(name, &data_bytes, mode.clone()) {
        info!("All checksums match for key '{}', no upload needed", name);

        // Final completion callback
        invoke_put_callback(&put_callback, PutEvent::Complete)
            .await
            .unwrap();

        return Ok(pads[0].address);
    }

    // If we get here, we need to upload the data but the pad count matches
    let encoding = if public {
        DATA_ENCODING_PUBLIC_DATA
    } else {
        DATA_ENCODING_PRIVATE_DATA
    };

    let context = Context {
        index: index.clone(),
        network: network.clone(),
        name: Arc::new(name.to_string()),
        data: data_bytes.clone(),
        chunk_ranges: Arc::new(chunk_ranges),
        public,
        encoding,
    };

    // Mark pads as Free to ensure they get uploaded
    let mut pads_to_write = pads.clone();
    for pad in &mut pads_to_write {
        pad.status = PadStatus::Free;
    }

    write_pipeline(context, pads_to_write, no_verify, put_callback.clone()).await?;

    if public {
        let (index_pad, index_data) = index.write().await.populate_index_pad(name)?;
        let index_data_bytes: Arc<Vec<u8>> = Arc::new(index_data);
        let index_chunk_ranges = Arc::new(vec![0..index_data_bytes.len()]);

        let index_pad_context = Context {
            index: index.clone(),
            network: network.clone(),
            name: Arc::new(name.to_string()),
            chunk_ranges: index_chunk_ranges,
            data: index_data_bytes,
            public,
            encoding: DATA_ENCODING_PUBLIC_INDEX,
        };

        // Call write_pipeline again for the single index pad
        write_pipeline(
            index_pad_context,
            vec![index_pad],
            no_verify,
            put_callback.clone(),
        )
        .await?;
    }

    Ok(pads[0].address)
}

pub async fn first_store(
    index: Arc<RwLock<crate::index::master_index::MasterIndex>>,
    network: Arc<Network>,
    name: &str,
    data_bytes: Arc<Vec<u8>>,
    mode: StorageMode,
    public: bool,
    no_verify: bool,
    put_callback: Option<PutCallback>,
) -> Result<ScratchpadAddress, Error> {
    let (pads, chunk_ranges) = index
        .write()
        .await
        .create_key(name, &data_bytes, mode, public)?;

    info!("Created key {} with {} pads", name, pads.len());

    let address = pads[0].address;

    let encoding = if public {
        DATA_ENCODING_PUBLIC_DATA
    } else {
        DATA_ENCODING_PRIVATE_DATA
    };

    let context = Context {
        index: index.clone(),
        network: network.clone(),
        name: Arc::new(name.to_string()),
        chunk_ranges: Arc::new(chunk_ranges),
        data: data_bytes.clone(),
        public,
        encoding,
    };

    write_pipeline(context, pads.clone(), no_verify, put_callback.clone()).await?;

    if public {
        let (index_pad, index_data) = index.write().await.populate_index_pad(name)?;
        let index_data_bytes: Arc<Vec<u8>> = Arc::new(index_data);
        let index_chunk_ranges = Arc::new(vec![0..index_data_bytes.len()]);

        let index_pad_context = Context {
            // Reuse index and network Arcs
            index: index.clone(),
            network: network.clone(),
            name: Arc::new(name.to_string()), // Reuse name Arc
            chunk_ranges: index_chunk_ranges,
            data: index_data_bytes,
            public, // Keep public flag
            encoding: DATA_ENCODING_PUBLIC_INDEX,
        };

        // Call write_pipeline again for the single index pad
        write_pipeline(
            index_pad_context,
            vec![index_pad],
            no_verify,
            put_callback.clone(), // Clone the callback Arc again
        )
        .await?;
    }

    // Final completion callback after all pipelines are done
    invoke_put_callback(&put_callback, PutEvent::Complete)
        .await
        .unwrap();

    Ok(address)
}
