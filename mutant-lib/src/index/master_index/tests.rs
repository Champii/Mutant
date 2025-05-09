use super::*;
use crate::config::NetworkChoice;
use crate::index::PadStatus;
use mutant_protocol::{StorageMode, MEDIUM_SCRATCHPAD_SIZE};
use std::path::PathBuf;

const DEFAULT_SCRATCHPAD_SIZE: usize = MEDIUM_SCRATCHPAD_SIZE;

// Helper to set up a temporary XDG directory for tests, always using Devnet
fn setup_test_environment() -> (PathBuf, MasterIndex) {
    let mutant_data_dir = utils::get_mutant_data_dir().unwrap();
    std::fs::remove_file(mutant_data_dir.join("master_index_devnet.cbor")).unwrap_or_default();

    let index = MasterIndex::new(NetworkChoice::Devnet);
    (mutant_data_dir, index)
}

#[test]
fn test_new_master_index() {
    let (_td, index) = setup_test_environment();
    assert!(index.index.is_empty());
    assert!(index.free_pads.is_empty());
    assert!(index.pending_verification_pads.is_empty());
}

#[test]
fn test_create_private_key_new() {
    let (_td, mut index) = setup_test_environment();
    let data = vec![0u8; DEFAULT_SCRATCHPAD_SIZE * 2 + 10]; // Data spanning more than 2 pads
    let key_name = "test_key";

    let (pads, _) = index
        .create_key(key_name, &data, StorageMode::Medium, false)
        .unwrap();

    assert_eq!(pads.len(), 3); // Should create 3 pads
    assert!(index.contains_key(key_name));
    if let Some(IndexEntry::PrivateKey(stored_pads)) = index.index.get(key_name) {
        assert_eq!(stored_pads.len(), 3);
        assert_eq!(stored_pads[0].status, PadStatus::Generated);
        assert_eq!(stored_pads[1].status, PadStatus::Generated);
        assert_eq!(stored_pads[2].status, PadStatus::Generated);
    } else {
        panic!("Key not found or not a PrivateKey entry");
    }
}

#[test]
fn test_create_private_key_existing() {
    let (_td, mut index) = setup_test_environment();
    let data = vec![1u8; 10];
    let key_name = "test_key";

    index
        .create_key(key_name, &data, StorageMode::Medium, false)
        .unwrap();
    let result = index.create_key(key_name, &data, StorageMode::Medium, false);

    assert!(matches!(
        result,
        Err(crate::error::Error::Index(crate::index::error::IndexError::KeyAlreadyExists(_)))
    ));
}

#[test]
fn test_update_pad_status_private() {
    let (_td, mut index) = setup_test_environment();
    let data = vec![0u8; DEFAULT_SCRATCHPAD_SIZE];
    let key_name = "test_key";
    let (pads, _) = index
        .create_key(key_name, &data, StorageMode::Medium, false)
        .unwrap();
    let pad_address = pads[0].address;

    index
        .update_pad_status(key_name, &pad_address, PadStatus::Confirmed, None)
        .unwrap();

    if let Some(IndexEntry::PrivateKey(updated_pads)) = index.index.get(key_name) {
        assert_eq!(updated_pads[0].status, PadStatus::Confirmed);
    } else {
        panic!("Key not found or not a PrivateKey entry");
    }
}

#[test]
fn test_contains_key() {
    let (_td, mut index) = setup_test_environment();
    let data = vec![1u8; 10];
    let key_name = "test_key";

    assert!(!index.contains_key(key_name));
    index
        .create_key(key_name, &data, StorageMode::Medium, false)
        .unwrap();
    assert!(index.contains_key(key_name));
    assert!(!index.contains_key("other_key"));
}

#[test]
fn test_remove_key_moves_pads() {
    let (_td, mut index) = setup_test_environment();
    let data_gen = vec![0u8; DEFAULT_SCRATCHPAD_SIZE];
    let data_conf = vec![1u8; DEFAULT_SCRATCHPAD_SIZE];
    let key_gen = "key_gen";
    let key_conf = "key_conf";

    let (pads_gen, _) = index
        .create_key(key_gen, &data_gen, StorageMode::Medium, false)
        .unwrap();
    let pad_gen_addr = pads_gen[0].address;

    let (pads_conf, _) = index
        .create_key(key_conf, &data_conf, StorageMode::Medium, false)
        .unwrap();
    let pad_conf_addr = pads_conf[0].address;
    index
        .update_pad_status(key_conf, &pad_conf_addr, PadStatus::Confirmed, None)
        .unwrap(); // Mark as non-generated

    // Remove the key with the 'Generated' pad
    index.remove_key(key_gen).unwrap();
    assert!(!index.contains_key(key_gen));
    assert_eq!(index.pending_verification_pads.len(), 1);
    assert_eq!(index.pending_verification_pads[0].address, pad_gen_addr);
    assert_eq!(index.free_pads.len(), 0);

    // Remove the key with the 'Confirmed' pad
    index.remove_key(key_conf).unwrap();
    assert!(!index.contains_key(key_conf));
    assert_eq!(index.pending_verification_pads.len(), 1); // Should still have the one from key_gen
    assert_eq!(index.free_pads.len(), 1);
    assert_eq!(index.free_pads[0].address, pad_conf_addr);
    assert_eq!(index.free_pads[0].status, PadStatus::Free);
}

#[test]
fn test_is_finished() {
    let (_td, mut index) = setup_test_environment();
    let data = vec![0u8; DEFAULT_SCRATCHPAD_SIZE * 2];
    let key_name = "test_key";

    let (pads, _) = index
        .create_key(key_name, &data, StorageMode::Medium, false)
        .unwrap();
    assert!(!index.is_finished(key_name)); // Not finished initially

    // Update one pad
    index
        .update_pad_status(key_name, &pads[0].address, PadStatus::Confirmed, None)
        .unwrap();
    assert!(!index.is_finished(key_name)); // Still not finished

    // Update the second pad
    index
        .update_pad_status(key_name, &pads[1].address, PadStatus::Confirmed, None)
        .unwrap();
    assert!(index.is_finished(key_name)); // Now finished

    assert!(!index.is_finished("non_existent_key")); // Test non-existent key
}

#[test]
fn test_verify_checksum_private() {
    let (_td, mut index) = setup_test_environment();
    let data = vec![0u8; DEFAULT_SCRATCHPAD_SIZE + 5];
    let key_name = "test_key";

    index
        .create_key(key_name, &data, StorageMode::Medium, false)
        .unwrap();

    // Verify with correct data
    assert!(index.verify_checksum(key_name, &data, StorageMode::Medium));

    // Verify with incorrect data (different length)
    let wrong_data_len = vec![0u8; DEFAULT_SCRATCHPAD_SIZE];
    assert!(!index.verify_checksum(key_name, &wrong_data_len, StorageMode::Medium));

    // Verify with incorrect data (same length, different content)
    let mut wrong_data_content = data.clone();
    wrong_data_content[0] = 1; // Change one byte
    assert!(!index.verify_checksum(key_name, &wrong_data_content, StorageMode::Medium));

    // Verify non-existent key
    assert!(!index.verify_checksum("non_existent_key", &data, StorageMode::Medium));
}

#[test]
fn test_list() {
    let (_td, mut index) = setup_test_environment();
    assert!(index.list().is_empty());

    index
        .create_key("key1", &[1], StorageMode::Medium, false)
        .unwrap();
    index
        .create_key("key2", &[2], StorageMode::Medium, false)
        .unwrap();

    let keys = index.list();

    assert_eq!(keys.keys().collect::<Vec<_>>(), vec!["key1", "key2"]);
}

#[test]
fn test_acquire_pads_reuse_free() {
    let (_td, mut index) = setup_test_environment();
    let data1 = vec![1u8; DEFAULT_SCRATCHPAD_SIZE];
    let data2 = vec![2u8; DEFAULT_SCRATCHPAD_SIZE];
    let key1 = "key1";
    let key2 = "key2";

    // Create first key, its pad gets generated
    let (pads1, _) = index
        .create_key(key1, &data1, StorageMode::Medium, false)
        .unwrap();
    assert_eq!(pads1.len(), 1);
    let pad1_addr = pads1[0].address;
    index
        .update_pad_status(key1, &pad1_addr, PadStatus::Confirmed, None)
        .unwrap(); // Mark as used

    // Remove first key, pad should go to free_pads
    index.remove_key(key1).unwrap();
    assert_eq!(index.free_pads.len(), 1);
    assert_eq!(index.pending_verification_pads.len(), 0);
    assert_eq!(index.free_pads[0].address, pad1_addr);
    assert_eq!(index.free_pads[0].status, PadStatus::Free); // Status updated

    // Create second key, should reuse the free pad
    let (pads2, _) = index
        .create_key(key2, &data2, StorageMode::Medium, false)
        .unwrap();
    assert_eq!(pads2.len(), 1);
    assert_eq!(pads2[0].address, pad1_addr); // Reused the same address
    assert_eq!(pads2[0].status, PadStatus::Free); // Status reset
    assert_eq!(pads2[0].checksum, PadInfo::checksum(&data2)); // Checksum updated
    assert!(index.free_pads.is_empty()); // Free pad list is now empty
}

#[test]
fn test_acquire_pads_generate_new() {
    let (_td, mut index) = setup_test_environment();
    let data = vec![1u8; DEFAULT_SCRATCHPAD_SIZE * 2];
    let key = "key";

    // No free pads initially
    assert!(index.free_pads.is_empty());

    let (pads, _) = index
        .create_key(key, &data, StorageMode::Medium, false)
        .unwrap();
    assert_eq!(pads.len(), 2);
    assert_ne!(pads[0].address, pads[1].address); // Ensure different addresses for generated pads
    assert!(index.free_pads.is_empty()); // Still no free pads
}

#[test]
fn test_acquire_pads_mix_free_and_new() {
    let (_td, mut index) = setup_test_environment();
    let data1 = vec![1u8; DEFAULT_SCRATCHPAD_SIZE];
    let data3 = vec![3u8; DEFAULT_SCRATCHPAD_SIZE * 3]; // Requires 3 pads
    let key1 = "key1";
    let key3 = "key3";

    // Create key1, mark pad as used, remove key -> pad goes to free_pads
    let (pads1, _) = index
        .create_key(key1, &data1, StorageMode::Medium, false)
        .unwrap();
    let pad1_addr = pads1[0].address;
    index
        .update_pad_status(key1, &pad1_addr, PadStatus::Confirmed, None)
        .unwrap();
    index.remove_key(key1).unwrap();
    assert_eq!(index.free_pads.len(), 1);

    // Create key3, requires 3 pads. Should use 1 free pad and generate 2 new ones.
    let (pads3, _) = index
        .create_key(key3, &data3, StorageMode::Medium, false)
        .unwrap();
    assert_eq!(pads3.len(), 3);
    assert!(index.free_pads.is_empty()); // Free pad was used

    // Check if the first pad reused the free one
    assert_eq!(pads3[0].address, pad1_addr);
    assert_eq!(pads3[0].status, PadStatus::Free);
    assert_eq!(
        pads3[0].checksum,
        PadInfo::checksum(&data3[0..DEFAULT_SCRATCHPAD_SIZE])
    );

    // Check the newly generated pads
    assert_ne!(pads3[1].address, pad1_addr);
    assert_ne!(pads3[2].address, pad1_addr);
    assert_eq!(pads3[1].status, PadStatus::Generated);
    assert_eq!(pads3[2].status, PadStatus::Generated);
    assert_eq!(
        pads3[1].checksum,
        PadInfo::checksum(&data3[DEFAULT_SCRATCHPAD_SIZE..DEFAULT_SCRATCHPAD_SIZE * 2])
    );
    assert_eq!(
        pads3[2].checksum,
        PadInfo::checksum(&data3[DEFAULT_SCRATCHPAD_SIZE * 2..])
    );

    if let Some(IndexEntry::PrivateKey(stored_pads)) = index.index.get(key3) {
        assert_eq!(stored_pads.len(), 3);
    } else {
        panic!("Key not found or not a PrivateKey entry");
    }
}

#[test]
fn test_load_non_existent_file() {
    // Test loading when the file doesn't exist for Devnet
    let (_td, index) = setup_test_environment(); // Sets up XDG env but doesn't create the file
    assert!(index.index.is_empty()); // Verify it's empty
}
