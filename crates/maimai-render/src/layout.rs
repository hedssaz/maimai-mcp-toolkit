pub(crate) const WIDTH: u32 = 1_920;
pub(crate) const HEADER_HEIGHT: u32 = 250;
pub(crate) const CARD_WIDTH: u32 = 352;
pub(crate) const CARD_HEIGHT: u32 = 158;
pub(crate) const CARD_GAP: u32 = 22;
pub(crate) const MARGIN_X: u32 = 56;
pub(crate) const COLUMNS: usize = 5;
pub(crate) const BOTTOM_MARGIN: u32 = 70;

pub(crate) fn canvas_height(card_count: usize) -> u32 {
    let rows = card_count.div_ceil(COLUMNS).max(1) as u32;
    HEADER_HEIGHT + rows * CARD_HEIGHT + rows.saturating_sub(1) * CARD_GAP + BOTTOM_MARGIN
}

pub(crate) fn card_origin(index: usize) -> (i32, i32) {
    let row = index / COLUMNS;
    let column = index % COLUMNS;
    let x = MARGIN_X as usize + column * (CARD_WIDTH + CARD_GAP) as usize;
    let y = HEADER_HEIGHT as usize + row * (CARD_HEIGHT + CARD_GAP) as usize;
    (x as i32, y as i32)
}
