//! Spatial grid model of the client windows.
//!
//! The tiler in [`super`] arranges `n` client windows on a grid of
//! `cols * rows` cells, with the final row possibly stretched to span
//! the full width when `n % cols != 0`. This module mirrors that layout
//! as a pure data structure so the enable/disable submenu can navigate
//! the cells spatially with arrow keys and `hjkl`.

#![deny(clippy::implicit_return)]
#![allow(clippy::needless_return, clippy::doc_overindented_list_items)]

use std::cmp::max;
use std::collections::HashMap;

use super::NavigationDirection;
use crate::utils::config::EdgeBehavior;

/// One client window's position on the spatial grid.
///
/// `col` is the leftmost upper-grid column the cell projects onto and
/// `col_span` is how many upper-grid columns it spans. Rows `0..rows-2`
/// are dense (`col_span == 1`); cells in a partial last row are stretched
/// (`col_span >= 1`).
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub(super) struct GridCell {
    /// Process id of the client owning this cell.
    pub pid: u32,
    /// 0-based row index.
    pub row: i32,
    /// Leftmost upper-grid column the cell projects onto.
    pub col: i32,
    /// Number of upper-grid columns the cell projects onto. Always
    /// `>= 1`; only `> 1` for partial-last-row cells.
    pub col_span: i32,
    /// 0-based position within the row, used for vertical roundtrips.
    pub pos_in_row: i32,
}

/// Spatial-grid view over the tracked client PIDs.
pub(super) struct ClientGrid {
    /// Number of columns in the dense upper rows.
    pub cols: i32,
    /// Total number of rows (`>= 1` whenever there is at least one cell).
    pub rows: i32,
    /// Cell count in the last row. `0` means the last row is also dense;
    /// otherwise `1..cols` last-row cells are stretched proportionally.
    last_row_count: i32,
    /// Cells sorted by `(row, col)` so the top-left cell is at index `0`.
    cells: Vec<GridCell>,
    /// PID lookup table.
    by_pid: HashMap<u32, usize>,
}

/// Compute the grid dimensions for `n` clients on a workspace with the
/// given aspect ratio.
///
/// Must match the formula used by the tiler in
/// [`super::determine_client_spatial_attributes`] so layout and
/// navigation stay in sync.
///
/// # Arguments
///
/// * `n`           - Number of client windows.
/// * `aspect`      - `workspace_width / workspace_height` (including
///                   frame padding) of the available area.
/// * `aspect_adj`  - The `aspect_ratio_adjustment` daemon config.
///
/// # Returns
///
/// `(cols, rows)`, each clamped to a minimum of `1`.
pub(super) fn grid_dimensions(n: i32, aspect: f64, aspect_adj: f64) -> (i32, i32) {
    let cols = max(((n as f64).sqrt() * (aspect + aspect_adj)) as i32, 1);
    let rows = max((n as f64 / cols as f64).ceil() as i32, 1);
    return (cols, rows);
}

impl ClientGrid {
    /// Build the grid from `(pid, tile_index)` pairs and the dimensions
    /// returned by [`grid_dimensions`] for the same `layout_n`.
    ///
    /// `tile_index` is the position each client was assigned the last
    /// time the tiler positioned its window. Surviving clients keep
    /// their `tile_index` across closures, so passing them here together
    /// with the layout's original `layout_n` produces a grid whose cells
    /// land at the same `(row, col)` the user sees on screen - with
    /// gaps where a window was closed but no retile has happened yet.
    ///
    /// # Arguments
    ///
    /// * `cells`     - `(pid, tile_index)` pairs for every surviving
    ///                 client.
    /// * `layout_n`  - The `number_of_consoles` the on-screen layout was
    ///                 last computed with. Used to derive the
    ///                 partial-last-row stretch.
    /// * `cols`      - Columns from [`grid_dimensions`] for `layout_n`.
    /// * `rows`      - Rows from [`grid_dimensions`] for `layout_n`.
    ///
    /// # Returns
    ///
    /// A populated [`ClientGrid`].
    pub(super) fn from_tiled_pids(
        cells: &[(u32, usize)],
        layout_n: i32,
        cols: i32,
        rows: i32,
    ) -> Self {
        let last_row_count = if cols > 0 { layout_n % cols } else { 0 };
        let mut grid_cells = Vec::with_capacity(cells.len());
        for &(pid, tile_index) in cells {
            let idx = tile_index as i32;
            let row = if cols > 0 { idx / cols } else { 0 };
            let pos_in_row = if cols > 0 { idx % cols } else { 0 };
            let (col, col_span) = if row == rows - 1 && last_row_count != 0 {
                let left = (pos_in_row * cols) / last_row_count;
                let right = ((pos_in_row + 1) * cols - 1) / last_row_count;
                (left, right - left + 1)
            } else {
                (pos_in_row, 1)
            };
            grid_cells.push(GridCell {
                pid,
                row,
                col,
                col_span,
                pos_in_row,
            });
        }
        grid_cells.sort_by_key(|c| return (c.row, c.col));
        let by_pid = grid_cells
            .iter()
            .enumerate()
            .map(|(i, c)| return (c.pid, i))
            .collect();
        return ClientGrid {
            cols,
            rows,
            last_row_count,
            cells: grid_cells,
            by_pid,
        };
    }

    /// Look up the cell owned by `pid`.
    ///
    /// # Arguments
    ///
    /// * `pid` - Process id to look up.
    ///
    /// # Returns
    ///
    /// `Some(&GridCell)` when present, `None` otherwise.
    pub(super) fn cell(&self, pid: u32) -> Option<&GridCell> {
        return self.by_pid.get(&pid).map(|&i| return &self.cells[i]);
    }

    /// PID of the top-left cell, or `None` for an empty grid. Used to
    /// re-anchor the submenu selection onto a sensible visual default.
    pub(super) fn top_left_pid(&self) -> Option<u32> {
        return self.cells.first().map(|c| return c.pid);
    }

    /// `true` when the grid has no cells.
    pub(super) fn is_empty(&self) -> bool {
        return self.cells.is_empty();
    }

    /// Compute the anchor column for a cell. Horizontal moves overwrite
    /// the in-flight anchor with the destination cell's anchor.
    ///
    /// Upper-row cells: their `col` (each cell occupies exactly one
    /// upper-grid column). Partial-last-row cells: the upper-grid column
    /// containing the cell's x-midpoint. The latter makes a Down + Up
    /// roundtrip return to the original cell from any starting point.
    ///
    /// # Arguments
    ///
    /// * `cell` - The destination cell.
    ///
    /// # Returns
    ///
    /// The anchor column for the cell.
    pub(super) fn anchor_for(&self, cell: &GridCell) -> i32 {
        if cell.row == self.rows - 1 && self.last_row_count != 0 {
            return ((2 * cell.pos_in_row + 1) * self.cols) / (2 * self.last_row_count);
        }
        return cell.col;
    }

    /// Compute the next selection after one navigation keystroke.
    ///
    /// # Arguments
    ///
    /// * `pid`        - Currently highlighted PID.
    /// * `anchor_col` - Anchor column carried from earlier moves.
    /// * `direction`  - Direction of the keystroke.
    /// * `edge`       - Behavior when the move would leave the grid.
    ///
    /// # Returns
    ///
    /// `Some((new_pid, new_anchor_col))` on a successful step.
    /// `None` when `pid` is not present in this grid (caller should
    /// re-anchor).
    pub(super) fn step(
        &self,
        pid: u32,
        anchor_col: i32,
        direction: NavigationDirection,
        edge: EdgeBehavior,
    ) -> Option<(u32, i32)> {
        let current = self.cell(pid)?;
        return match direction {
            NavigationDirection::Left | NavigationDirection::Right => {
                self.step_horizontal(current, anchor_col, direction, edge)
            }
            NavigationDirection::Up | NavigationDirection::Down => {
                Some(self.step_vertical(current, anchor_col, direction, edge))
            }
        };
    }

    /// Horizontal step within `current.row`. Returns `None` only when
    /// the row somehow contains no cells (cannot happen for a valid
    /// `current` looked up from the grid). A clamped no-op preserves
    /// the in-flight `anchor_col` so a subsequent vertical step still
    /// targets the column the user originally carried over.
    fn step_horizontal(
        &self,
        current: &GridCell,
        anchor_col: i32,
        direction: NavigationDirection,
        edge: EdgeBehavior,
    ) -> Option<(u32, i32)> {
        let mut row_cells: Vec<&GridCell> = self
            .cells
            .iter()
            .filter(|c| return c.row == current.row)
            .collect();
        row_cells.sort_by_key(|c| return c.col);
        let pos = row_cells.iter().position(|c| return c.pid == current.pid)?;
        let next = match direction {
            NavigationDirection::Left => {
                if pos == 0 {
                    match edge {
                        EdgeBehavior::Clamp => return Some((current.pid, anchor_col)),
                        EdgeBehavior::Wrap => *row_cells.last()?,
                    }
                } else {
                    row_cells[pos - 1]
                }
            }
            NavigationDirection::Right => {
                if pos + 1 >= row_cells.len() {
                    match edge {
                        EdgeBehavior::Clamp => return Some((current.pid, anchor_col)),
                        EdgeBehavior::Wrap => *row_cells.first()?,
                    }
                } else {
                    row_cells[pos + 1]
                }
            }
            _ => return None,
        };
        return Some((next.pid, self.anchor_for(next)));
    }

    /// Vertical step into the target row, preserving the in-flight
    /// `anchor_col`.
    fn step_vertical(
        &self,
        current: &GridCell,
        anchor_col: i32,
        direction: NavigationDirection,
        edge: EdgeBehavior,
    ) -> (u32, i32) {
        let target_row = match direction {
            NavigationDirection::Up => {
                if current.row == 0 {
                    match edge {
                        EdgeBehavior::Clamp => return (current.pid, anchor_col),
                        EdgeBehavior::Wrap => self.rows - 1,
                    }
                } else {
                    current.row - 1
                }
            }
            NavigationDirection::Down => {
                if current.row + 1 >= self.rows {
                    match edge {
                        EdgeBehavior::Clamp => return (current.pid, anchor_col),
                        EdgeBehavior::Wrap => 0,
                    }
                } else {
                    current.row + 1
                }
            }
            _ => return (current.pid, anchor_col),
        };
        let row_cells: Vec<&GridCell> = self
            .cells
            .iter()
            .filter(|c| return c.row == target_row)
            .collect();
        if row_cells.is_empty() {
            return (current.pid, anchor_col);
        }
        let is_partial_last_row = target_row == self.rows - 1 && self.last_row_count != 0;
        let best = row_cells
            .into_iter()
            .min_by_key(|c| {
                return (
                    self.anchor_distance(c, anchor_col, is_partial_last_row),
                    c.col,
                );
            })
            .expect("row_cells just checked non-empty");
        return (best.pid, anchor_col);
    }

    /// Spatial distance between a cell and an anchor column, used to
    /// pick the target on a vertical step. Dense rows reduce to
    /// `|c.col - anchor|`; partial-last-row cells use their stretched
    /// x-extent so the cell whose midpoint is closest to the anchor's
    /// centerline wins. The result is in arbitrary integer units valid
    /// only for comparisons within the same row.
    fn anchor_distance(&self, cell: &GridCell, anchor_col: i32, is_partial_last_row: bool) -> i64 {
        if is_partial_last_row {
            let cell_mid = (2 * cell.pos_in_row as i64 + 1) * self.cols as i64;
            let anchor_mid = (2 * anchor_col as i64 + 1) * self.last_row_count as i64;
            return (cell_mid - anchor_mid).abs();
        }
        return (2 * cell.col as i64 - 2 * anchor_col as i64).abs();
    }
}
