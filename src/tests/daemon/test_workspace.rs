mod daemon_workspace_test {
    use crate::daemon::workspace::get_workspace_area;
    use crate::utils::windows::MockWindowsApi;
    use windows::Win32::UI::WindowsAndMessaging::{
        SM_CXFIXEDFRAME, SM_CXMAXIMIZED, SM_CXSIZEFRAME, SM_CYFIXEDFRAME, SM_CYMAXIMIZED,
        SM_CYSIZEFRAME,
    };

    #[test]
    fn test_get_workspace_area() {
        // Arrange
        let mut mock_windows_api = MockWindowsApi::new();

        // Mock system metrics values
        let expected_width = 1920;
        let expected_height = 1080;
        let expected_x_fixed_frame = 3;
        let expected_y_fixed_frame = 3;
        let expected_x_size_frame = 4;
        let expected_y_size_frame = 4;
        let daemon_console_height = 200;

        // Set up mock expectations
        mock_windows_api
            .expect_get_system_metrics()
            .with(mockall::predicate::eq(SM_CXMAXIMIZED))
            .times(1)
            .returning(move |_| return expected_width);

        mock_windows_api
            .expect_get_system_metrics()
            .with(mockall::predicate::eq(SM_CYMAXIMIZED))
            .times(1)
            .returning(move |_| return expected_height);

        mock_windows_api
            .expect_get_system_metrics()
            .with(mockall::predicate::eq(SM_CXFIXEDFRAME))
            .times(1)
            .returning(move |_| return expected_x_fixed_frame);

        mock_windows_api
            .expect_get_system_metrics()
            .with(mockall::predicate::eq(SM_CYFIXEDFRAME))
            .times(1)
            .returning(move |_| return expected_y_fixed_frame);

        mock_windows_api
            .expect_get_system_metrics()
            .with(mockall::predicate::eq(SM_CXSIZEFRAME))
            .times(1)
            .returning(move |_| return expected_x_size_frame);

        mock_windows_api
            .expect_get_system_metrics()
            .with(mockall::predicate::eq(SM_CYSIZEFRAME))
            .times(1)
            .returning(move |_| return expected_y_size_frame);

        // Act
        let workspace_area = get_workspace_area(&mock_windows_api, daemon_console_height);

        // Assert
        assert_eq!(workspace_area.x, 0);
        assert_eq!(workspace_area.y, 0);
        assert_eq!(workspace_area.width, expected_width - 1); // Function subtracts 1
        assert_eq!(
            workspace_area.height,
            expected_height - 1 - daemon_console_height
        ); // Function subtracts 1 and daemon height
        assert_eq!(workspace_area.x_fixed_frame, expected_x_fixed_frame);
        assert_eq!(workspace_area.y_fixed_frame, expected_y_fixed_frame);
        assert_eq!(workspace_area.x_size_frame, expected_x_size_frame);
        assert_eq!(workspace_area.y_size_frame, expected_y_size_frame);
    }
}
