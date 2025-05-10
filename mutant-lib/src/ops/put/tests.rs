#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use tokio::sync::RwLock;
    use rand::RngCore;

    use crate::index::master_index::{IndexEntry, MasterIndex};
    use crate::index::{PadInfo, PadStatus};
    use crate::network::{Network, NetworkChoice};
    use crate::ops::put::operations::{first_store, update};
    use mutant_protocol::StorageMode;

    // Helper function to generate random data
    fn generate_random_data(size: usize) -> Vec<u8> {
        let mut data = vec![0u8; size];
        rand::thread_rng().fill_bytes(&mut data);
        data
    }

    // Helper function to setup test environment
    async fn setup_test_environment() -> (Arc<RwLock<MasterIndex>>, Arc<Network>) {
        let network = Arc::new(
            Network::new(
                crate::network::DEV_TESTNET_PRIVATE_KEY_HEX,
                NetworkChoice::Devnet,
            )
            .expect("Failed to create network"),
        );
        let index = Arc::new(RwLock::new(MasterIndex::new(NetworkChoice::Devnet)));
        (index, network)
    }

    // Helper function to compare data chunks
    fn compare_chunks(original: &[u8], updated: &[u8], mode: StorageMode) -> (usize, usize) {
        let chunk_size = mode.scratchpad_size();
        let original_chunks: Vec<&[u8]> = original.chunks(chunk_size).collect();
        let updated_chunks: Vec<&[u8]> = updated.chunks(chunk_size).collect();

        let mut unchanged_count = 0;
        let mut changed_count = 0;

        for (i, updated_chunk) in updated_chunks.iter().enumerate() {
            if i < original_chunks.len() && PadInfo::checksum(original_chunks[i]) == PadInfo::checksum(updated_chunk) {
                unchanged_count += 1;
            } else {
                changed_count += 1;
            }
        }

        (unchanged_count, changed_count)
    }

    #[tokio::test]
    async fn test_update_same_size_private() {
        let (index, network) = setup_test_environment().await;
        let key_name = "test_update_same_size";
        let initial_data = Arc::new(generate_random_data(1024));
        let mode = StorageMode::Medium;

        // First store the initial data
        let initial_result = first_store(
            index.clone(),
            network.clone(),
            key_name,
            initial_data.clone(),
            mode.clone(),
            false, // private
            false, // verify
            None,  // no callback
        )
        .await;

        assert!(initial_result.is_ok(), "Initial store failed");

        // Get the initial pads
        let initial_pads = index.read().await.get_pads(key_name);
        assert!(!initial_pads.is_empty(), "No pads found for initial store");

        // Create updated data with some changes but same size
        let mut updated_data = initial_data.as_ref().clone();
        // Modify part of the data to ensure some chunks change
        for i in 0..100 {
            updated_data[i] = 0xFF;
        }
        let updated_data = Arc::new(updated_data);

        // Calculate how many chunks should change
        let (_unchanged_count, changed_count) = compare_chunks(&initial_data, &updated_data, mode.clone());

        // Update the key
        let update_result = update(
            index.clone(),
            network.clone(),
            key_name,
            updated_data.clone(),
            mode.clone(),
            false, // private
            false, // verify
            None,  // no callback
        )
        .await;

        assert!(update_result.is_ok(), "Update failed");

        // Get the updated pads
        let updated_pads = index.read().await.get_pads(key_name);

        // Verify the number of pads is the same
        assert_eq!(
            initial_pads.len(),
            updated_pads.len(),
            "Number of pads changed after update"
        );

        // Count how many pads were actually updated
        let updated_count = updated_pads
            .iter()
            .filter(|p| p.last_known_counter > 0)
            .count();

        // Verify that only the changed chunks were updated
        assert_eq!(
            changed_count, updated_count,
            "Expected {} pads to be updated, but {} were updated",
            changed_count, updated_count
        );

        // Verify we can retrieve the updated data
        let retrieved_data = index.read().await.verify_checksum(key_name, &updated_data, mode.clone());
        assert!(retrieved_data, "Retrieved data doesn't match updated data");
    }

    #[tokio::test]
    async fn test_update_larger_size_private() {
        let (index, network) = setup_test_environment().await;
        let key_name = "test_update_larger_size";

        // Use a small initial size that will fit in a single pad
        let chunk_size = StorageMode::Medium.scratchpad_size();
        let initial_data = Arc::new(generate_random_data(chunk_size / 2));
        let mode = StorageMode::Medium;

        // First store the initial data
        let initial_result = first_store(
            index.clone(),
            network.clone(),
            key_name,
            initial_data.clone(),
            mode.clone(),
            false, // private
            false, // verify
            None,  // no callback
        )
        .await;

        assert!(initial_result.is_ok(), "Initial store failed");

        // Get the initial pads
        let initial_pads = index.read().await.get_pads(key_name);
        let initial_pad_count = initial_pads.len();
        assert_eq!(initial_pad_count, 1, "Expected initial data to use exactly 1 pad");

        // Create updated data with larger size that will require multiple pads
        let updated_data = Arc::new(generate_random_data(chunk_size * 3)); // Use 3 pads

        // Update the key
        let update_result = update(
            index.clone(),
            network.clone(),
            key_name,
            updated_data.clone(),
            mode.clone(),
            false, // private
            false, // verify
            None,  // no callback
        )
        .await;

        assert!(update_result.is_ok(), "Update failed");

        // Get the updated pads
        let updated_pads = index.read().await.get_pads(key_name);

        // Verify the number of pads increased
        assert!(
            updated_pads.len() > initial_pad_count,
            "Number of pads did not increase after update with larger data. Initial: {}, Updated: {}",
            initial_pad_count,
            updated_pads.len()
        );

        // Verify we can retrieve the updated data
        let retrieved_data = index.read().await.verify_checksum(key_name, &updated_data, mode.clone());
        assert!(retrieved_data, "Retrieved data doesn't match updated data");
    }

    #[tokio::test]
    async fn test_update_smaller_size_private() {
        let (index, network) = setup_test_environment().await;
        let key_name = "test_update_smaller_size";

        // Use a large initial size that will require multiple pads
        let chunk_size = StorageMode::Medium.scratchpad_size();
        let initial_data = Arc::new(generate_random_data(chunk_size * 3)); // Use 3 pads
        let mode = StorageMode::Medium;

        // First store the initial data
        let initial_result = first_store(
            index.clone(),
            network.clone(),
            key_name,
            initial_data.clone(),
            mode.clone(),
            false, // private
            false, // verify
            None,  // no callback
        )
        .await;

        assert!(initial_result.is_ok(), "Initial store failed");

        // Get the initial pads
        let initial_pads = index.read().await.get_pads(key_name);
        let initial_pad_count = initial_pads.len();
        assert!(initial_pad_count > 1, "Expected initial data to use multiple pads");

        // Create updated data with smaller size that will fit in a single pad
        let updated_data = Arc::new(generate_random_data(chunk_size / 2)); // Use 1 pad

        // Update the key
        let update_result = update(
            index.clone(),
            network.clone(),
            key_name,
            updated_data.clone(),
            mode.clone(),
            false, // private
            false, // verify
            None,  // no callback
        )
        .await;

        assert!(update_result.is_ok(), "Update failed");

        // Get the updated pads
        let updated_pads = index.read().await.get_pads(key_name);

        // Verify the number of pads decreased
        assert!(
            updated_pads.len() < initial_pad_count,
            "Number of pads did not decrease after update with smaller data. Initial: {}, Updated: {}",
            initial_pad_count,
            updated_pads.len()
        );

        // Verify we can retrieve the updated data
        let retrieved_data = index.read().await.verify_checksum(key_name, &updated_data, mode.clone());
        assert!(retrieved_data, "Retrieved data doesn't match updated data");
    }

    #[tokio::test]
    async fn test_update_public_key_preserve_index_pad() {
        let (index, network) = setup_test_environment().await;
        let key_name = "test_update_public_preserve_index";
        let initial_data = Arc::new(generate_random_data(1024));
        let mode = StorageMode::Medium;

        // First store the initial data as public
        let initial_result = first_store(
            index.clone(),
            network.clone(),
            key_name,
            initial_data.clone(),
            mode.clone(),
            true, // public
            false, // verify
            None,  // no callback
        )
        .await;

        assert!(initial_result.is_ok(), "Initial public store failed");

        // Get the initial entry to extract the index pad
        let initial_index_pad = match index.read().await.get_entry(key_name) {
            Some(IndexEntry::PublicUpload(index_pad, _)) => Some(index_pad.clone()),
            _ => None,
        };

        assert!(initial_index_pad.is_some(), "No index pad found for public key");
        let initial_index_pad = initial_index_pad.unwrap();

        // Create updated data
        let updated_data = Arc::new(generate_random_data(1536)); // 1.5x the size

        // Update the key
        let update_result = update(
            index.clone(),
            network.clone(),
            key_name,
            updated_data.clone(),
            mode.clone(),
            true, // public
            false, // verify
            None,  // no callback
        )
        .await;

        assert!(update_result.is_ok(), "Public update failed");

        // Get the updated entry to check if the index pad was preserved
        let updated_index_pad = match index.read().await.get_entry(key_name) {
            Some(IndexEntry::PublicUpload(index_pad, _)) => Some(index_pad.clone()),
            _ => None,
        };

        assert!(updated_index_pad.is_some(), "No index pad found after update");
        let updated_index_pad = updated_index_pad.unwrap();

        // Verify the index pad address was preserved
        assert_eq!(
            initial_index_pad.address, updated_index_pad.address,
            "Public index pad address changed after update"
        );

        // Verify the counter was incremented
        assert!(
            updated_index_pad.last_known_counter > initial_index_pad.last_known_counter,
            "Public index pad counter was not incremented"
        );

        // Verify we can retrieve the updated data
        let retrieved_data = index.read().await.verify_checksum(key_name, &updated_data, mode.clone());
        assert!(retrieved_data, "Retrieved data doesn't match updated data");
    }

    #[tokio::test]
    async fn test_update_with_unchanged_chunks() {
        let (index, network) = setup_test_environment().await;
        let key_name = "test_update_unchanged_chunks";
        let initial_data = Arc::new(generate_random_data(2048));
        let mode = StorageMode::Medium;

        // First store the initial data
        let initial_result = first_store(
            index.clone(),
            network.clone(),
            key_name,
            initial_data.clone(),
            mode.clone(),
            false, // private
            false, // verify
            None,  // no callback
        )
        .await;

        assert!(initial_result.is_ok(), "Initial store failed");

        // Get the initial pads
        let initial_pads = index.read().await.get_pads(key_name);

        // Create updated data with only some chunks changed
        let mut updated_data = initial_data.as_ref().clone();

        // Only modify the first chunk, leave the rest unchanged
        let chunk_size = mode.scratchpad_size();
        for i in 0..chunk_size.min(100) {
            updated_data[i] = 0xFF;
        }

        let updated_data = Arc::new(updated_data);

        // Update the key
        let update_result = update(
            index.clone(),
            network.clone(),
            key_name,
            updated_data.clone(),
            mode.clone(),
            false, // private
            false, // verify
            None,  // no callback
        )
        .await;

        assert!(update_result.is_ok(), "Update failed");

        // Get the updated pads
        let updated_pads = index.read().await.get_pads(key_name);

        // Verify only the first pad was updated (has a higher counter)
        assert!(
            updated_pads[0].last_known_counter > initial_pads[0].last_known_counter,
            "First pad was not updated"
        );

        // Verify the other pads were not updated (same counter)
        for i in 1..updated_pads.len() {
            assert_eq!(
                updated_pads[i].last_known_counter, initial_pads[i].last_known_counter,
                "Pad {} was unnecessarily updated", i
            );
        }

        // Verify we can retrieve the updated data
        let retrieved_data = index.read().await.verify_checksum(key_name, &updated_data, mode.clone());
        assert!(retrieved_data, "Retrieved data doesn't match updated data");
    }

    #[tokio::test]
    async fn test_update_with_additional_pads_confirmation() {
        let (index, network) = setup_test_environment().await;
        let key_name = "test_update_additional_pads_confirmation";

        // Use a small initial size that will fit in a single pad
        let chunk_size = StorageMode::Medium.scratchpad_size();
        let initial_data = Arc::new(generate_random_data(chunk_size / 2));
        let mode = StorageMode::Medium;

        // First store the initial data
        let initial_result = first_store(
            index.clone(),
            network.clone(),
            key_name,
            initial_data.clone(),
            mode.clone(),
            false, // private
            false, // verify
            None,  // no callback
        )
        .await;

        assert!(initial_result.is_ok(), "Initial store failed");

        // Get the initial pads
        let initial_pads = index.read().await.get_pads(key_name);
        let initial_pad_count = initial_pads.len();
        assert_eq!(initial_pad_count, 1, "Expected initial data to use exactly 1 pad");

        // Create updated data with larger size that will require multiple pads
        // Use a size that will require 3 pads to ensure we're testing with multiple additional pads
        let updated_data = Arc::new(generate_random_data(chunk_size * 3));

        // Update the key
        let update_result = update(
            index.clone(),
            network.clone(),
            key_name,
            updated_data.clone(),
            mode.clone(),
            false, // private
            false, // verify
            None,  // no callback
        )
        .await;

        assert!(update_result.is_ok(), "Update failed");

        // Get the updated pads
        let updated_pads = index.read().await.get_pads(key_name);

        // Verify the number of pads increased
        assert_eq!(
            updated_pads.len(), 3,
            "Expected exactly 3 pads after update, got {}",
            updated_pads.len()
        );

        // Verify all pads are in Confirmed status
        for (i, pad) in updated_pads.iter().enumerate() {
            assert_eq!(
                pad.status, PadStatus::Confirmed,
                "Pad {} is not in Confirmed status: {:?}",
                i, pad.status
            );
        }

        // Verify we can retrieve the updated data
        let retrieved_data = index.read().await.verify_checksum(key_name, &updated_data, mode.clone());
        assert!(retrieved_data, "Retrieved data doesn't match updated data");
    }
}
