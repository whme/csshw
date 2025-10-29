//! Tests for daemon workspace functionality

use crate::daemon::workspace::{get_workspace_area, WorkspaceArea};

#[cfg(test)]
mod workspace_test {
    use super::*;

    #[test]
    fn test_get_workspace_area_basic() {
        // Test basic workspace area calculation
        let daemon_height = 100;
        let workspace = get_workspace_area(daemon_height);

        // Verify the structure is properly initialized
        assert_eq!(workspace.x, 0);
        assert_eq!(workspace.y, 0);

        // Width and height should be positive values from Windows API
        assert!(workspace.width > 0);
        assert!(workspace.height > 0);

        // Frame values should be non-negative
        assert!(workspace.x_fixed_frame >= 0);
        assert!(workspace.y_fixed_frame >= 0);
        assert!(workspace.x_size_frame >= 0);
        assert!(workspace.y_size_frame >= 0);
    }

    #[test]
    fn test_get_workspace_area_daemon_height_subtraction() {
        // Test that daemon height is properly subtracted
        let daemon_height_small = 50;
        let daemon_height_large = 200;

        let workspace_small = get_workspace_area(daemon_height_small);
        let workspace_large = get_workspace_area(daemon_height_large);

        // The workspace with larger daemon height should have smaller available height
        assert!(workspace_small.height > workspace_large.height);
        assert_eq!(
            workspace_small.height - workspace_large.height,
            daemon_height_large - daemon_height_small
        );

        // Other dimensions should remain the same
        assert_eq!(workspace_small.x, workspace_large.x);
        assert_eq!(workspace_small.y, workspace_large.y);
        assert_eq!(workspace_small.width, workspace_large.width);
    }

    #[test]
    fn test_get_workspace_area_zero_daemon_height() {
        // Test with zero daemon height
        let workspace = get_workspace_area(0);

        assert_eq!(workspace.x, 0);
        assert_eq!(workspace.y, 0);
        assert!(workspace.width > 0);
        assert!(workspace.height > 0);
    }

    #[test]
    fn test_get_workspace_area_negative_daemon_height() {
        // Test with negative daemon height (should increase available height)
        let negative_height = -50;
        let zero_height = 0;

        let workspace_negative = get_workspace_area(negative_height);
        let workspace_zero = get_workspace_area(zero_height);

        // Negative daemon height should result in larger available height
        assert!(workspace_negative.height > workspace_zero.height);
        assert_eq!(
            workspace_negative.height - workspace_zero.height,
            -negative_height
        );
    }

    #[test]
    fn test_workspace_area_struct_properties() {
        // Test WorkspaceArea struct properties
        let workspace = WorkspaceArea {
            x: 10,
            y: 20,
            width: 1920,
            height: 1080,
            x_fixed_frame: 5,
            y_fixed_frame: 5,
            x_size_frame: 3,
            y_size_frame: 3,
        };

        assert_eq!(workspace.x, 10);
        assert_eq!(workspace.y, 20);
        assert_eq!(workspace.width, 1920);
        assert_eq!(workspace.height, 1080);
        assert_eq!(workspace.x_fixed_frame, 5);
        assert_eq!(workspace.y_fixed_frame, 5);
        assert_eq!(workspace.x_size_frame, 3);
        assert_eq!(workspace.y_size_frame, 3);
    }

    #[test]
    fn test_workspace_area_clone_copy() {
        // Test that WorkspaceArea implements Clone and Copy
        let original = WorkspaceArea {
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
            x_fixed_frame: 5,
            y_fixed_frame: 5,
            x_size_frame: 3,
            y_size_frame: 3,
        };

        let cloned = original;
        let copied = original;

        assert_eq!(original.x, cloned.x);
        assert_eq!(original.width, cloned.width);
        assert_eq!(original.x, copied.x);
        assert_eq!(original.width, copied.width);
    }

    #[test]
    fn test_workspace_area_debug() {
        // Test that WorkspaceArea implements Debug
        let workspace = WorkspaceArea {
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
            x_fixed_frame: 5,
            y_fixed_frame: 5,
            x_size_frame: 3,
            y_size_frame: 3,
        };

        let debug_str = format!("{workspace:?}");
        assert!(debug_str.contains("WorkspaceArea"));
        assert!(debug_str.contains("1920"));
        assert!(debug_str.contains("1080"));
    }
}
