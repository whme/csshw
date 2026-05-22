The `[e]nable/disable input` control-mode submenu now navigates the
client tiles as a real 2D grid: arrow keys and `hjkl` move the
highlight one cell in the requested direction, including across the
partial-last-row boundary where cells span multiple upper-row columns.
A Down + Up roundtrip returns to the starting cell because an anchor
column is carried across vertical moves. When a client window has
closed without a retile, surviving windows keep their on-screen
positions and the navigation grid follows that visible layout - a
vertical step into the gap snaps to the nearest surviving cell in the
target row. The new `daemon.submenu_edge_behavior` key in
`csshw-config.toml` selects what happens when the move would leave the
grid: `clamp` (default, keeps the current selection) or `wrap` (wraps
to the opposite edge of the same row or column).
