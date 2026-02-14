//! Client registry for tracking clients in insertion order with support for deletions

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use windows::Win32::Foundation::{HANDLE, HWND};

/// Representation of a client
#[derive(Clone)]
pub struct Client {
    /// Hostname the client is connect to (or supposed to connect to).
    pub hostname: String,
    /// Window handle to the clients console window.
    pub window_handle: HWND,
    /// Process handle to the client process.
    pub process_handle: HANDLE,
}

unsafe impl Send for Client {}

/// Entry in the client registry
#[derive(Clone)]
struct RegistryEntry {
    /// The client data
    client: Client,
    /// Whether this entry has been deleted
    deleted: bool,
}

/// A registry for tracking clients that supports:
/// - Iteration in insertion order
/// - Indexing by insertion index
/// - Thread-safe operations
/// - Deletions without affecting iteration order
pub struct ClientRegistry {
    /// Entries stored in insertion order
    entries: Vec<RegistryEntry>,
    /// Map from insertion index to vector index for fast lookups
    index_map: HashMap<usize, usize>,
    /// The next insertion index to use
    next_index: usize,
}

impl ClientRegistry {
    /// Creates a new empty client registry
    pub fn new() -> Self {
        return Self {
            entries: Vec::new(),
            index_map: HashMap::new(),
            next_index: 0,
        };
    }

    /// Inserts a client into the registry and returns its insertion index
    pub fn insert(&mut self, client: Client) -> usize {
        let index = self.next_index;
        self.next_index += 1;

        let entry = RegistryEntry {
            client,
            deleted: false,
        };

        self.entries.push(entry);
        self.index_map.insert(index, self.entries.len() - 1);

        return index;
    }

    /// Removes a client by its insertion index
    /// Returns true if the client was removed, false if not found
    pub fn remove(&mut self, index: usize) -> bool {
        if let Some(&vec_index) = self.index_map.get(&index) {
            self.entries[vec_index].deleted = true;
            self.index_map.remove(&index);
            return true;
        }
        return false;
    }

    /// Returns the number of active (non-deleted) clients
    pub fn len(&self) -> usize {
        return self.index_map.len();
    }

    /// Returns true if the registry is empty
    pub fn is_empty(&self) -> bool {
        return self.index_map.is_empty();
    }

    /// Iterates over all active clients in insertion order
    pub fn iter(&self) -> impl Iterator<Item = &Client> {
        return self
            .entries
            .iter()
            .filter(|entry| return !entry.deleted)
            .map(|entry| return &entry.client);
    }

    /// Retains only the clients that satisfy the predicate
    pub fn retain<F>(&mut self, mut f: F)
    where
        F: FnMut(&Client) -> bool,
    {
        let indices_to_remove: Vec<usize> = self
            .index_map
            .iter()
            .filter_map(|(&index, &vec_index)| {
                let entry = &self.entries[vec_index];
                if !f(&entry.client) {
                    return Some(index);
                } else {
                    return None;
                }
            })
            .collect();

        for index in indices_to_remove {
            self.remove(index);
        }
    }
}

impl Default for ClientRegistry {
    fn default() -> Self {
        return Self::new();
    }
}

/// Thread-safe wrapper around ClientRegistry
pub type SharedClientRegistry = Arc<Mutex<ClientRegistry>>;

/// Creates a new shared client registry
pub fn new_shared_registry() -> SharedClientRegistry {
    return Arc::new(Mutex::new(ClientRegistry::new()));
}

#[cfg(test)]
#[path = "../tests/daemon/test_client_registry.rs"]
mod test_client_registry;
