#![allow(dead_code)]

#[derive(Clone, Copy, Debug)]
pub struct BoardMasks {
    pub width: u32,
    pub height: u32,
    pub board_mask: u128,
    pub left_col_mask: u128,
    pub right_col_mask: u128,
    pub top_row_mask: u128,
}

impl BoardMasks {
    pub fn new(width: u32, height: u32) -> Self {
        let n = (width * height) as u128;
        assert!(n <= 128, "Board {}x{} too large for u128 bitboard", width, height);

        let board_mask = if n == 128 { u128::MAX } else { (1u128 << n) - 1 };

        let mut left_col_mask = 0u128;
        for row in 0..height {
            left_col_mask |= 1u128 << (row * width);
        }

        let right_col_mask = left_col_mask << (width - 1);

        let mut top_row_mask = 0u128;
        for col in 0..width {
            top_row_mask |= 1u128 << ((height - 1) * width + col);
        }

        BoardMasks { width, height, board_mask, left_col_mask, right_col_mask, top_row_mask }
    }

    pub fn standard() -> Self {
        Self::new(11, 11)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Hash)]
pub struct Bitboard(pub u128);

impl Bitboard {
    pub const EMPTY: Bitboard = Bitboard(0);

    #[inline]
    pub fn from_coord(x: i32, y: i32, width: u32) -> Bitboard {
        Bitboard(1u128 << ((y as u32) * width + (x as u32)))
    }

    #[inline]
    pub fn from_idx(idx: u32) -> Bitboard {
        Bitboard(1u128 << idx)
    }

    #[inline]
    pub fn set_coord(self, x: i32, y: i32, width: u32) -> Bitboard {
        Bitboard(self.0 | (1u128 << ((y as u32) * width + (x as u32))))
    }

    #[inline]
    pub fn clear_coord(self, x: i32, y: i32, width: u32) -> Bitboard {
        Bitboard(self.0 & !(1u128 << ((y as u32) * width + (x as u32))))
    }

    #[inline]
    pub fn test_coord(self, x: i32, y: i32, width: u32) -> bool {
        self.0 & (1u128 << ((y as u32) * width + (x as u32))) != 0
    }

    #[inline]
    pub fn set_idx(self, idx: u32) -> Bitboard {
        Bitboard(self.0 | (1u128 << idx))
    }

    #[inline]
    pub fn clear_idx(self, idx: u32) -> Bitboard {
        Bitboard(self.0 & !(1u128 << idx))
    }

    #[inline]
    pub fn test_idx(self, idx: u32) -> bool {
        (self.0 >> idx) & 1 != 0
    }

    #[inline]
    pub fn popcount(self) -> u32 {
        self.0.count_ones()
    }

    #[inline]
    pub fn is_empty(self) -> bool {
        self.0 == 0
    }

    #[inline]
    pub fn any(self) -> bool {
        self.0 != 0
    }

    #[inline]
    pub fn expand_up(self, m: &BoardMasks) -> Bitboard {
        Bitboard((self.0 << m.width) & m.board_mask)
    }

    #[inline]
    pub fn expand_down(self, m: &BoardMasks) -> Bitboard {
        Bitboard(self.0 >> m.width)
    }

    #[inline]
    pub fn expand_left(self, m: &BoardMasks) -> Bitboard {
        Bitboard((self.0 & !m.left_col_mask) >> 1)
    }

    #[inline]
    pub fn expand_right(self, m: &BoardMasks) -> Bitboard {
        Bitboard((self.0 & !m.right_col_mask) << 1)
    }

    #[inline]
    pub fn expand(self, m: &BoardMasks) -> Bitboard {
        Bitboard(
            self.expand_up(m).0
                | self.expand_down(m).0
                | self.expand_left(m).0
                | self.expand_right(m).0,
        )
    }

    pub fn flood_fill(self, blocked: Bitboard, m: &BoardMasks) -> Bitboard {
        let open = Bitboard(m.board_mask & !blocked.0);
        let mut curr = Bitboard(self.0 & open.0);
        loop {
            let next = Bitboard((curr.expand(m).0 | curr.0) & open.0);
            if next.0 == curr.0 {
                break;
            }
            curr = next;
        }
        curr
    }

    pub fn flood_fill_with_target(
        self,
        blocked: Bitboard,
        target: Bitboard,
        m: &BoardMasks,
    ) -> (Bitboard, bool) {
        let open = Bitboard(m.board_mask & !blocked.0);
        let mut curr = Bitboard(self.0 & open.0);
        loop {
            let next = Bitboard((curr.expand(m).0 | curr.0) & open.0);
            if next.0 == curr.0 {
                break;
            }
            curr = next;
        }
        let has_target = (curr.0 & target.0) != 0;
        (curr, has_target)
    }
}

impl std::ops::BitOr for Bitboard {
    type Output = Self;
    #[inline]
    fn bitor(self, rhs: Self) -> Self {
        Bitboard(self.0 | rhs.0)
    }
}

impl std::ops::BitAnd for Bitboard {
    type Output = Self;
    #[inline]
    fn bitand(self, rhs: Self) -> Self {
        Bitboard(self.0 & rhs.0)
    }
}

impl std::ops::BitXor for Bitboard {
    type Output = Self;
    #[inline]
    fn bitxor(self, rhs: Self) -> Self {
        Bitboard(self.0 ^ rhs.0)
    }
}

impl std::ops::Not for Bitboard {
    type Output = Self;
    #[inline]
    fn not(self) -> Self {
        Bitboard(!self.0)
    }
}

impl std::ops::BitOrAssign for Bitboard {
    #[inline]
    fn bitor_assign(&mut self, rhs: Self) {
        self.0 |= rhs.0;
    }
}

impl std::ops::BitAndAssign for Bitboard {
    #[inline]
    fn bitand_assign(&mut self, rhs: Self) {
        self.0 &= rhs.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_flood_fill_open_board() {
        let m = BoardMasks::new(11, 11);
        let start = Bitboard::from_coord(5, 5, 11);
        let result = start.flood_fill(Bitboard::EMPTY, &m);
        assert_eq!(result.popcount(), 121, "All 121 cells should be reachable on empty board");
    }

    #[test]
    fn test_flood_fill_horizontal_wall() {
        let m = BoardMasks::new(11, 11);
        let start = Bitboard::from_coord(0, 0, 11);
        let mut blocked = Bitboard::EMPTY;
        for x in 0..11 {
            blocked = blocked.set_coord(x, 5, 11);
        }
        let result = start.flood_fill(blocked, &m);
        assert_eq!(result.popcount(), 55);
    }

    #[test]
    fn test_no_column_wrap_right() {
        let m = BoardMasks::new(11, 11);
        let bb = Bitboard::from_coord(10, 5, 11);
        let expanded = bb.expand(&m);
        assert!(expanded.test_coord(9, 5, 11));
        assert!(expanded.test_coord(10, 4, 11));
        assert!(expanded.test_coord(10, 6, 11));
        assert!(!expanded.test_coord(0, 6, 11), "Must not wrap to next row");
    }

    #[test]
    fn test_no_column_wrap_left() {
        let m = BoardMasks::new(11, 11);
        let bb = Bitboard::from_coord(0, 5, 11);
        let expanded = bb.expand(&m);
        assert!(expanded.test_coord(1, 5, 11));
        assert!(expanded.test_coord(0, 4, 11));
        assert!(expanded.test_coord(0, 6, 11));
        assert!(!expanded.test_coord(10, 4, 11), "Must not wrap to previous row");
    }

    #[test]
    fn test_board_mask_popcount() {
        let m = BoardMasks::new(11, 11);
        assert_eq!(Bitboard(m.board_mask).popcount(), 121);
    }
}
