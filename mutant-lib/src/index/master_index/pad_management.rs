use crate::error::Error;
use crate::index::error::IndexError;
use crate::index::pad_info::PadInfo;
use crate::storage::ScratchpadAddress;
use mutant_protocol::StorageMode;
use std::ops::Range;

use super::{IndexEntry, MasterIndex};

impl MasterIndex {
    pub fn chunk_data(&self, data_bytes: &[u8], mode: StorageMode) -> Vec<Range<usize>> {
        let pad_size = mode.scratchpad_size();
        let mut ranges = Vec::new();
        let mut current_pos = 0;

        while current_pos < data_bytes.len() {
            let end = std::cmp::min(current_pos + pad_size, data_bytes.len());
            ranges.push(current_pos..end);
            current_pos = end;
        }

        ranges
    }

    pub fn acquire_pads(
        &mut self,
        data_bytes: &[u8],
        chunk_ranges: &[Range<usize>],
    ) -> Result<Vec<PadInfo>, Error> {
        self.generate_pads(data_bytes, chunk_ranges.iter())
    }

    pub fn generate_pads<'a>(
        &mut self,
        data_bytes: &'a [u8],
        chunk_ranges: impl Iterator<Item = &'a Range<usize>>,
    ) -> Result<Vec<PadInfo>, Error> {
        let ranges: Vec<_> = chunk_ranges.cloned().collect();
        let num_pads_needed = ranges.len();

        // Acquire pads using the internal helper
        let mut available_pads = self._acquire_pads_internal(num_pads_needed)?;

        let mut generated_pads = Vec::with_capacity(num_pads_needed);
        for (i, range) in ranges.iter().enumerate() {
            let chunk_data_slice = &data_bytes[range.clone()];
            let mut pad_info = available_pads.remove(0);

            pad_info = pad_info.update_data(chunk_data_slice, i);
            generated_pads.push(pad_info);
        }

        // // Add any remaining unused acquired pads back to the free list
        // self.free_pads.extend(available_pads);

        if !available_pads.is_empty() {
            return Err(Error::Internal("Unexpected remaining pads detected. This indicates a bug.".to_string()));
        }

        Ok(generated_pads)
    }

    /// Internal helper function to acquire a specified number of pads,
    /// prioritizing generation of new pads before using free ones.
    pub(crate) fn _acquire_pads_internal(&mut self, num_pads_needed: usize) -> Result<Vec<PadInfo>, Error> {
        // Calculate how many pads to generate and how many to take from the free list
        let free_pads_count = self.free_pads.len();
        let pads_to_generate = num_pads_needed.saturating_sub(free_pads_count);
        let pads_to_take_from_free = num_pads_needed - pads_to_generate;

        // Take the required pads from the free list first
        if self.free_pads.len() < pads_to_take_from_free {
            return Err(Error::Internal(format!(
                "Insufficient free pads available. Needed {}, have {}",
                pads_to_take_from_free,
                self.free_pads.len()
            )));
        }

        let taken_free_pads: Vec<_> = self.free_pads.drain(..pads_to_take_from_free).collect();

        // Then, generate the remaining required new pads
        let mut generated_new_pads = Vec::with_capacity(pads_to_generate);
        for _ in 0..pads_to_generate {
            // Create a dummy PadInfo; size and checksum will be set later
            generated_new_pads.push(PadInfo::new(&[], 0));
        }

        // Combine taken and generated pads (Free first, then New)
        let mut available_pads = taken_free_pads;
        available_pads.append(&mut generated_new_pads);

        if available_pads.len() < num_pads_needed {
            return Err(Error::Internal(format!(
                "Failed to acquire enough pads. Needed {}, got {}",
                num_pads_needed,
                available_pads.len()
            )));
        }

        available_pads.iter_mut().for_each(|p| {
            p.last_known_counter += 1;
        });

        Ok(available_pads)
    }


    /// Update a key with a new set of pads
    pub fn update_key_with_pads(
        &mut self,
        key_name: &str,
        pads: Vec<PadInfo>,
        index_pad: Option<PadInfo>,
    ) -> Result<(), Error> {
        if !self.index.contains_key(key_name) {
            return Err(Error::Index(IndexError::KeyNotFound(key_name.to_string())));
        }

        // Update the key with the new pads
        if let Some(index_pad) = index_pad {
            // For public keys, update with the index pad
            self.index.insert(
                key_name.to_string(),
                IndexEntry::PublicUpload(index_pad, pads),
            );
        } else {
            // For private keys, just update with the new pads
            self.index.insert(key_name.to_string(), IndexEntry::PrivateKey(pads));
        }

        // Save the updated index
        self.save(self.network_choice)?;

        Ok(())
    }

    pub async fn recycle_errored_pad(
        &mut self,
        key_name: &str,
        pad_address: &ScratchpadAddress,
    ) -> Result<PadInfo, Error> {
        let mut new_pad = if self.free_pads.is_empty() {
            // If no free pads, generate a new one temporarily.
            // The actual data/checksum doesn't matter here as it will be overwritten.
            PadInfo::new(&[0u8; 1], 0)
        } else {
            // Use a pad from the free list
            self.free_pads.pop().unwrap()
        };

        if let Some(entry) = self.index.get_mut(key_name) {
            let result = match entry {
                super::IndexEntry::PrivateKey(pads) => {
                    if let Some(pad_index) = pads.iter().position(|p| p.address == *pad_address) {
                        let old_pad = pads[pad_index].clone();

                        // Configure the new pad based on the old one
                        Self::update_pad_properties(&mut new_pad, &old_pad);
                        // new_pad already has its new address and sk_bytes

                        // Replace the old pad info in the vector
                        pads[pad_index] = new_pad.clone();

                        self.pending_verification_pads.push(old_pad);
                        Ok(new_pad)
                    } else {
                        Err(IndexError::KeyNotFound(format!(
                            "Pad address {} not found in key {}",
                            pad_address, key_name
                        )))
                    }
                }
                super::IndexEntry::PublicUpload(index_pad, pads) => {
                    if *pad_address == index_pad.address {
                        // Recycling the index pad itself
                        let old_pad = index_pad.clone();

                        // Configure the new pad based on the old one
                        Self::update_pad_properties(&mut new_pad, &old_pad);

                        // Replace the index pad info
                        *index_pad = new_pad.clone();

                        self.pending_verification_pads.push(old_pad);
                        Ok(new_pad)
                    } else {
                        // Recycling a data pad
                        if let Some(data_pad_index) =
                            pads.iter().position(|p| p.address == *pad_address)
                        {
                            let old_pad = pads[data_pad_index].clone();

                            // Configure the new pad info based on the old one
                            Self::update_pad_properties(&mut new_pad, &old_pad);
                            // new_pad already has its new address and sk_bytes

                            // Replace the old pad info in the vector
                            pads[data_pad_index] = new_pad.clone();

                            self.pending_verification_pads.push(old_pad);
                            Ok(new_pad)
                        } else {
                            Err(IndexError::KeyNotFound(format!(
                                "Pad address {} not found in key {}",
                                pad_address, key_name
                            )))
                        }
                    }
                }
            };

            // Save the index if the operation inside the match was successful
            match result {
                Ok(ref pad_info) => {
                    // Use ref here to avoid moving pad_info
                    self.save(self.network_choice)?;
                    Ok(pad_info.clone()) // Clone the result to return
                }
                Err(e) => Err(e.into()), // Propagate index error as Error
            }
        } else {
            Err(IndexError::KeyNotFound(key_name.to_string()).into())
        }
    }

    pub fn pad_exists(&self, pad_address: &ScratchpadAddress) -> bool {
        let address_exists = self
            .free_pads
            .iter()
            .chain(self.pending_verification_pads.iter())
            .any(|p| p.address == *pad_address);

        let index_exists = self.index.iter().any(|(_, entry)| match entry {
            super::IndexEntry::PrivateKey(pads) => pads.iter().any(|p| p.address == *pad_address),
            super::IndexEntry::PublicUpload(index_pad, pads) => {
                index_pad.address == *pad_address || pads.iter().any(|p| p.address == *pad_address)
            }
        });

        address_exists || index_exists
    }

    /// Helper function to update a pad's properties based on another pad
    /// This is used when recycling pads or updating pad information
    pub(crate) fn update_pad_properties(target_pad: &mut PadInfo, source_pad: &PadInfo) {
        target_pad.checksum = source_pad.checksum;
        target_pad.size = source_pad.size;
        target_pad.chunk_index = source_pad.chunk_index;
    }
}
