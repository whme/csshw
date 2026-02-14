//! Tests for the client registry module

mod client_registry_test {
    use crate::daemon::client_registry::{Client, ClientRegistry};
    use windows::Win32::Foundation::{HANDLE, HWND};

    fn create_test_client(hostname: &str) -> Client {
        return Client {
            hostname: hostname.to_string(),
            window_handle: HWND(std::ptr::null_mut()),
            process_handle: HANDLE(std::ptr::null_mut()),
        };
    }

    #[test]
    fn test_insert_and_length() {
        let mut registry = ClientRegistry::new();

        let idx0 = registry.insert(create_test_client("host1"));
        let idx1 = registry.insert(create_test_client("host2"));

        assert_eq!(idx0, 0);
        assert_eq!(idx1, 1);
        assert_eq!(registry.len(), 2);

        let hostnames: Vec<String> = registry.iter().map(|c| return c.hostname.clone()).collect();
        assert_eq!(hostnames, vec!["host1", "host2"]);
    }

    #[test]
    fn test_remove() {
        let mut registry = ClientRegistry::new();

        let _idx0 = registry.insert(create_test_client("host1"));
        let idx1 = registry.insert(create_test_client("host2"));
        let _idx2 = registry.insert(create_test_client("host3"));

        assert_eq!(registry.len(), 3);

        registry.remove(idx1);

        assert_eq!(registry.len(), 2);
        let hostnames: Vec<String> = registry.iter().map(|c| return c.hostname.clone()).collect();
        assert_eq!(hostnames, vec!["host1", "host3"]);
    }

    #[test]
    fn test_iteration_order() {
        let mut registry = ClientRegistry::new();

        let _idx0 = registry.insert(create_test_client("host1"));
        let _idx1 = registry.insert(create_test_client("host2"));
        let _idx2 = registry.insert(create_test_client("host3"));
        let hostnames: Vec<String> = registry.iter().map(|c| return c.hostname.clone()).collect();

        assert_eq!(hostnames, vec!["host1", "host2", "host3"]);
    }

    #[test]
    fn test_iteration_order_after_deletion() {
        let mut registry = ClientRegistry::new();

        let _idx0 = registry.insert(create_test_client("host1"));
        let idx1 = registry.insert(create_test_client("host2"));
        let _idx2 = registry.insert(create_test_client("host3"));

        registry.remove(idx1);

        let hostnames: Vec<String> = registry.iter().map(|c| return c.hostname.clone()).collect();

        // Insertion order preserved, but host2 is gone
        assert_eq!(hostnames, vec!["host1", "host3"]);
    }

    #[test]
    fn test_retain() {
        let mut registry = ClientRegistry::new();

        registry.insert(create_test_client("host1"));
        registry.insert(create_test_client("host2"));
        registry.insert(create_test_client("host3"));

        registry.retain(|client| return client.hostname != "host2");

        assert_eq!(registry.len(), 2);
        let hostnames: Vec<String> = registry.iter().map(|c| return c.hostname.clone()).collect();
        assert_eq!(hostnames, vec!["host1", "host3"]);
    }

    #[test]
    fn test_multiple_insert_delete_cycles() {
        let mut registry = ClientRegistry::new();

        // Insert 3 items
        let idx0 = registry.insert(create_test_client("host1"));
        let idx1 = registry.insert(create_test_client("host2"));
        let idx2 = registry.insert(create_test_client("host3"));

        assert_eq!(registry.len(), 3);
        let hostnames: Vec<String> = registry.iter().map(|c| return c.hostname.clone()).collect();
        assert_eq!(hostnames, vec!["host1", "host2", "host3"]);

        // Delete middle item
        registry.remove(idx1);
        assert_eq!(registry.len(), 2);
        let hostnames: Vec<String> = registry.iter().map(|c| return c.hostname.clone()).collect();
        assert_eq!(hostnames, vec!["host1", "host3"]);

        // Insert another item - should appear at the end in iteration order
        let idx3 = registry.insert(create_test_client("host4"));
        assert_eq!(registry.len(), 3);
        let hostnames: Vec<String> = registry.iter().map(|c| return c.hostname.clone()).collect();
        assert_eq!(hostnames, vec!["host1", "host3", "host4"]);

        // Delete first item
        registry.remove(idx0);
        assert_eq!(registry.len(), 2);
        let hostnames: Vec<String> = registry.iter().map(|c| return c.hostname.clone()).collect();
        assert_eq!(hostnames, vec!["host3", "host4"]);

        // Insert another item
        let idx4 = registry.insert(create_test_client("host5"));
        assert_eq!(registry.len(), 3);
        let hostnames: Vec<String> = registry.iter().map(|c| return c.hostname.clone()).collect();
        assert_eq!(hostnames, vec!["host3", "host4", "host5"]);

        // Verify we can still remove by the correct indices
        registry.remove(idx2);
        assert_eq!(registry.len(), 2);
        let hostnames: Vec<String> = registry.iter().map(|c| return c.hostname.clone()).collect();
        assert_eq!(hostnames, vec!["host4", "host5"]);

        // Verify indices that should work
        assert_eq!(idx3, 3);
        assert_eq!(idx4, 4);
    }

    #[test]
    fn test_insert_after_multiple_deletes() {
        let mut registry = ClientRegistry::new();

        // Insert 5 items
        let idx0 = registry.insert(create_test_client("host1"));
        let idx1 = registry.insert(create_test_client("host2"));
        let idx2 = registry.insert(create_test_client("host3"));
        let idx3 = registry.insert(create_test_client("host4"));
        let idx4 = registry.insert(create_test_client("host5"));

        // Delete items 1 and 3
        registry.remove(idx1);
        registry.remove(idx3);

        assert_eq!(registry.len(), 3);
        let hostnames: Vec<String> = registry.iter().map(|c| return c.hostname.clone()).collect();
        assert_eq!(hostnames, vec!["host1", "host3", "host5"]);

        // Insert new items
        let idx5 = registry.insert(create_test_client("host6"));
        let idx6 = registry.insert(create_test_client("host7"));

        assert_eq!(registry.len(), 5);
        let hostnames: Vec<String> = registry.iter().map(|c| return c.hostname.clone()).collect();
        assert_eq!(hostnames, vec!["host1", "host3", "host5", "host6", "host7"]);

        // Verify we can still access and remove using correct indices
        registry.remove(idx0);
        registry.remove(idx2);
        registry.remove(idx4);

        assert_eq!(registry.len(), 2);
        let hostnames: Vec<String> = registry.iter().map(|c| return c.hostname.clone()).collect();
        assert_eq!(hostnames, vec!["host6", "host7"]);

        // Ensure indices are sequential
        assert_eq!(idx5, 5);
        assert_eq!(idx6, 6);
    }

    #[test]
    fn test_entries_vec_stays_in_sync() {
        let mut registry = ClientRegistry::new();

        // Insert 3 items - should be at vec positions 0, 1, 2
        let idx0 = registry.insert(create_test_client("host1"));
        let idx1 = registry.insert(create_test_client("host2"));
        let idx2 = registry.insert(create_test_client("host3"));

        // Internal state check: entries vec should have 3 items
        assert_eq!(registry.entries.len(), 3);
        assert_eq!(registry.index_map.len(), 3);

        // Remove middle item (idx1)
        registry.remove(idx1);

        // Internal state: entries vec still has 3 items, but index_map has 2
        assert_eq!(registry.entries.len(), 3);
        assert_eq!(registry.index_map.len(), 2);

        // The entry at position 1 should be marked as deleted
        assert!(registry.entries[1].deleted);
        assert_eq!(registry.entries[1].client.hostname, "host2");

        // Insert a new item - should go to vec position 3
        let idx3 = registry.insert(create_test_client("host4"));

        // Internal state: entries vec has 4 items, index_map has 3
        assert_eq!(registry.entries.len(), 4);
        assert_eq!(registry.index_map.len(), 3);

        // Verify the mapping is correct
        assert_eq!(*registry.index_map.get(&idx0).unwrap(), 0);
        assert_eq!(*registry.index_map.get(&idx2).unwrap(), 2);
        assert_eq!(*registry.index_map.get(&idx3).unwrap(), 3);

        // Iteration should give us host1, host3, host4
        let hostnames: Vec<String> = registry.iter().map(|c| return c.hostname.clone()).collect();
        assert_eq!(hostnames, vec!["host1", "host3", "host4"]);
    }

    #[test]
    fn test_retain_with_subsequent_operations() {
        let mut registry = ClientRegistry::new();

        // Insert 5 items
        let _idx0 = registry.insert(create_test_client("host1"));
        let _idx1 = registry.insert(create_test_client("host2"));
        let _idx2 = registry.insert(create_test_client("host3"));
        let _idx3 = registry.insert(create_test_client("host4"));
        let _idx4 = registry.insert(create_test_client("host5"));

        // Use retain to remove some items
        registry.retain(|client| {
            return client.hostname == "host1"
                || client.hostname == "host3"
                || client.hostname == "host5";
        });

        assert_eq!(registry.len(), 3);
        let hostnames: Vec<String> = registry.iter().map(|c| return c.hostname.clone()).collect();
        assert_eq!(hostnames, vec!["host1", "host3", "host5"]);

        // Now insert more items
        let idx5 = registry.insert(create_test_client("host6"));
        let idx6 = registry.insert(create_test_client("host7"));

        assert_eq!(registry.len(), 5);
        let hostnames: Vec<String> = registry.iter().map(|c| return c.hostname.clone()).collect();
        assert_eq!(hostnames, vec!["host1", "host3", "host5", "host6", "host7"]);

        // Verify indices are correct
        assert_eq!(idx5, 5);
        assert_eq!(idx6, 6);

        // Internal state should show entries vector has grown but has deleted entries
        assert_eq!(registry.entries.len(), 7); // 5 original + 2 new
        assert_eq!(registry.index_map.len(), 5); // Only 5 active
    }
}
