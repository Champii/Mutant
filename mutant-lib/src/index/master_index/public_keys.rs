use crate::error::Error;
use crate::index::error::IndexError;
use crate::index::pad_info::PadInfo;
use log::info;

use super::{IndexEntry, MasterIndex};

impl MasterIndex {
    /// Extracts the index pad from a public key, if it exists
    ///
    /// This is used when updating a public key to preserve the public index pad
    pub fn extract_public_index_pad(&self, key_name: &str) -> Option<PadInfo> {
        if let Some(entry) = self.index.get(key_name) {
            if let IndexEntry::PublicUpload(index_pad, _) = entry {
                return Some(index_pad.clone());
            }
        }
        None
    }

    /// Updates a public key with a new set of data pads but preserves the original index pad
    ///
    /// This is critical for public keys because the index pad address must remain the same
    /// to maintain accessibility of the key through its public address.
    pub fn update_public_key_with_preserved_index_pad(
        &mut self,
        key_name: &str,
        preserved_index_pad: PadInfo,
    ) -> Result<(), Error> {
        if !self.contains_key(key_name) {
            return Err(IndexError::KeyNotFound(key_name.to_string()).into());
        }

        if let Some(entry) = self.index.get_mut(key_name) {
            if let IndexEntry::PublicUpload(index_pad, _) = entry {
                *index_pad = preserved_index_pad;
            } else {
                return Err(IndexError::KeyNotFound(key_name.to_string()).into());
            }
        }

        self.save(self.network_choice)?;

        info!("Updated public key {} with preserved index pad", key_name);

        Ok(())
    }

    pub fn populate_index_pad(&mut self, key_name: &str) -> Result<(PadInfo, Vec<u8>), Error> {
        let res = match self.index.get_mut(key_name) {
            Some(IndexEntry::PublicUpload(index_pad, pads)) => {
                let index_data = serde_cbor::to_vec(&pads).unwrap();

                index_pad.size = index_data.len();
                index_pad.checksum = PadInfo::checksum(&index_data);

                Ok((index_pad.clone(), index_data))
            }
            _ => Err(IndexError::KeyNotFound(key_name.to_string()).into()),
        };

        self.save(self.network_choice)?;

        res
    }
}
