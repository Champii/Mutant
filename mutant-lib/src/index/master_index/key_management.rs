use crate::error::Error;
use crate::index::error::IndexError;
use crate::index::PadStatus;
use log::{debug, info};
use mutant_protocol::StorageMode;
use std::ops::Range;

use super::{IndexEntry, MasterIndex};

impl MasterIndex {
    pub fn create_key(
        &mut self,
        key_name: &str,
        data_bytes: &[u8],
        mode: StorageMode,
        public: bool,
    ) -> Result<(Vec<super::PadInfo>, Vec<Range<usize>>), Error> {
        if self.index.contains_key(key_name) {
            return Err(IndexError::KeyAlreadyExists(key_name.to_string()).into());
        }

        let chunk_ranges = self.chunk_data(data_bytes, mode);
        let pads = self.acquire_pads(data_bytes, &chunk_ranges)?;

        if public {
            // Empty index pad info for now
            let mut index_pad_info = self._acquire_pads_internal(1)?[0].clone();
            // Explicitly set chunk_index to 0 for the index pad, regardless of whether it's new or reused.
            index_pad_info.chunk_index = 0;

            self.index.insert(
                key_name.to_string(),
                IndexEntry::PublicUpload(index_pad_info, pads.clone()),
            );
        } else {
            self.index
                .insert(key_name.to_string(), IndexEntry::PrivateKey(pads.clone()));
        }

        self.save(self.network_choice)?;

        Ok((pads, chunk_ranges))
    }

    pub fn remove_key(&mut self, key_name: &str) -> Result<(), Error> {
        // for each pad that has a status different than Generated, we update their status to Free
        let mut pads_to_free = Vec::new();
        let mut pads_to_verify = Vec::new();

        if let Some(entry) = self.index.get_mut(key_name) {
            if let IndexEntry::PrivateKey(pads) = entry {
                pads.iter_mut().for_each(|p| {
                    if p.status != PadStatus::Generated {
                        p.status = PadStatus::Free;
                        pads_to_free.push(p.clone());
                    } else {
                        pads_to_verify.push(p.clone());
                    }
                    p.checksum = 0;
                    p.size = 0;
                });
            } else if let IndexEntry::PublicUpload(index, pads) = entry {
                if index.status != PadStatus::Generated {
                    index.status = PadStatus::Free;
                    pads_to_free.push(index.clone());
                } else {
                    pads_to_verify.push(index.clone());
                }
                pads.iter_mut().for_each(|p| {
                    if p.status != PadStatus::Generated {
                        p.status = PadStatus::Free;
                        pads_to_free.push(p.clone());
                    } else {
                        pads_to_verify.push(p.clone());
                    }
                    p.checksum = 0;
                    p.size = 0;
                });
            }
        }

        debug!(
            "Removing key {} with {} pads to free and {} pads to verify",
            key_name,
            pads_to_free.len(),
            pads_to_verify.len()
        );

        self.free_pads.extend(pads_to_free);
        self.pending_verification_pads.extend(pads_to_verify);

        self.index.remove(key_name);

        self.save(self.network_choice)?;

        info!("Removed key {}", key_name);

        Ok(())
    }

    pub fn contains_key(&self, key_name: &str) -> bool {
        self.index.contains_key(key_name)
    }

    pub fn get_pads(&self, key_name: &str) -> Vec<super::PadInfo> {
        if let Some(entry) = self.index.get(key_name) {
            match entry {
                IndexEntry::PrivateKey(pads) => pads.clone(),
                IndexEntry::PublicUpload(_index, pads) => pads.clone(),
            }
        } else {
            Vec::new()
        }
    }

    pub fn get_entry(&self, key_name: &str) -> Option<&IndexEntry> {
        self.index.get(key_name)
    }

    pub fn add_entry(&mut self, key_name: &str, entry: IndexEntry) -> Result<(), Error> {
        self.index.insert(key_name.to_string(), entry);
        self.save(self.network_choice)?;
        Ok(())
    }

    pub fn update_entry(&mut self, key_name: &str, entry: IndexEntry) -> Result<bool, Error> {
        // check if the key exists, and only update if the counter of the first pad (or index pad for public keys) is higher than the local one
        if let Some(existing_entry) = self.index.get_mut(key_name) {
            match existing_entry {
                IndexEntry::PrivateKey(existing_pads) => match &entry {
                    IndexEntry::PrivateKey(pads) => {
                        if pads[0].last_known_counter > existing_pads[0].last_known_counter {
                            *existing_entry = entry;
                            self.save(self.network_choice)?;
                            Ok(true)
                        } else {
                            Ok(false)
                        }
                    }
                    _ => panic!("Cannot update public key with private key"),
                },
                IndexEntry::PublicUpload(existing_index, _existing_pads) => match &entry {
                    IndexEntry::PublicUpload(index, _pads) => {
                        if index.last_known_counter > existing_index.last_known_counter {
                            *existing_entry = entry;
                            self.save(self.network_choice)?;
                            Ok(true)
                        } else {
                            Ok(false)
                        }
                    }
                    _ => panic!("Cannot update private key with public key"),
                },
            }
        } else {
            Err(IndexError::KeyNotFound(key_name.to_string()).into())
        }
    }

    pub fn is_public(&self, key_name: &str) -> bool {
        self.index.get(key_name).map_or(false, |entry| match entry {
            IndexEntry::PublicUpload(_, _) => true,
            IndexEntry::PrivateKey(_) => false,
        })
    }

    pub fn list(&self) -> std::collections::BTreeMap<String, IndexEntry> {
        let mut keys = self.index.clone();
        // put all the secret keys in the entries to 0
        keys.iter_mut().for_each(|(_, entry)| {
            if let IndexEntry::PrivateKey(pads) = entry {
                pads.iter_mut().for_each(|p| p.sk_bytes = vec![0; 32]);
            } else if let IndexEntry::PublicUpload(index, pads) = entry {
                pads.iter_mut().for_each(|p| p.sk_bytes = vec![0; 32]);
                index.sk_bytes = vec![0; 32];
            }
        });
        keys
    }
}
