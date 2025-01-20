mod daemon_test {
    use std::ffi::c_void;

    use windows::Win32::Foundation::HWND;

    use crate::{
        daemon::{resolve_cluster_tags, HWNDWrapper},
        utils::config::Cluster,
    };

    #[test]
    fn test_hwnd_wrapper_equality() {
        assert_eq!(
            HWNDWrapper {
                hwdn: HWND(1 as *mut c_void)
            },
            HWNDWrapper {
                hwdn: HWND(1 as *mut c_void)
            }
        );
        assert_ne!(
            HWNDWrapper {
                hwdn: HWND(1 as *mut c_void)
            },
            HWNDWrapper {
                hwdn: HWND(2 as *mut c_void)
            }
        );
    }

    #[test]
    fn test_resolve_cluster_tags() {
        let hosts: Vec<&str> = vec!["host0", "cluster1", "host3", "host0", "host1"];
        let clusters: Vec<Cluster> = vec![Cluster {
            name: "cluster1".to_string(),
            hosts: vec!["host1".to_string(), "host2".to_string()],
        }];
        assert_eq!(
            resolve_cluster_tags(hosts, &clusters),
            vec!["host0", "host1", "host2", "host3", "host0", "host1"]
        );
    }

    #[test]
    fn test_resolve_cluster_tags_no_cluster() {
        let hosts: Vec<&str> = vec!["host0"];
        let clusters: Vec<Cluster> = vec![Cluster {
            name: "cluster1".to_string(),
            hosts: vec!["host1".to_string(), "host2".to_string()],
        }];
        assert_eq!(resolve_cluster_tags(hosts, &clusters), vec!["host0"]);
    }

    #[test]
    fn test_resolve_cluster_tags_simple_nested_cluster() {
        let hosts: Vec<&str> = vec!["cluster2"];
        let clusters: Vec<Cluster> = vec![
            Cluster {
                name: "cluster1".to_string(),
                hosts: vec!["host1".to_string(), "host2".to_string()],
            },
            Cluster {
                name: "cluster2".to_string(),
                hosts: vec!["cluster1".to_owned(), "host3".to_owned()],
            },
        ];
        assert_eq!(
            resolve_cluster_tags(hosts, &clusters),
            vec!["host1", "host2", "host3"]
        );
    }
}
