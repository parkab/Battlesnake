#![allow(dead_code)]

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Direction {
    Up,
    Down,
    Left,
    Right,
}

impl Direction {
    pub const ALL: [Direction; 4] = [
        Direction::Up,
        Direction::Down,
        Direction::Left,
        Direction::Right,
    ];

    pub fn to_str(self) -> &'static str {
        match self {
            Direction::Up => "up",
            Direction::Down => "down",
            Direction::Left => "left",
            Direction::Right => "right",
        }
    }

    pub fn dx(self) -> i32 {
        match self {
            Direction::Left => -1,
            Direction::Right => 1,
            _ => 0,
        }
    }

    pub fn dy(self) -> i32 {
        match self {
            Direction::Up => 1,
            Direction::Down => -1,
            _ => 0,
        }
    }

    pub fn to_index(self) -> usize {
        match self {
            Direction::Up => 0,
            Direction::Down => 1,
            Direction::Left => 2,
            Direction::Right => 3,
        }
    }

    pub fn from_index(i: usize) -> Direction {
        match i {
            0 => Direction::Up,
            1 => Direction::Down,
            2 => Direction::Left,
            _ => Direction::Right,
        }
    }

    pub fn opposite(self) -> Direction {
        match self {
            Direction::Up => Direction::Down,
            Direction::Down => Direction::Up,
            Direction::Left => Direction::Right,
            Direction::Right => Direction::Left,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash, Default)]
pub struct Coord {
    pub x: i32,
    pub y: i32,
}

impl Coord {
    pub fn new(x: i32, y: i32) -> Self {
        Coord { x, y }
    }

    pub fn step(self, dir: Direction) -> Coord {
        Coord {
            x: self.x + dir.dx(),
            y: self.y + dir.dy(),
        }
    }

    pub fn manhattan(self, other: Coord) -> i32 {
        (self.x - other.x).abs() + (self.y - other.y).abs()
    }

    pub fn to_idx(self, width: i32) -> usize {
        (self.y * width + self.x) as usize
    }

    pub fn from_idx(idx: usize, width: i32) -> Coord {
        Coord {
            x: (idx as i32) % width,
            y: (idx as i32) / width,
        }
    }

    pub fn is_in_bounds(self, width: i32, height: i32) -> bool {
        self.x >= 0 && self.x < width && self.y >= 0 && self.y < height
    }
}

impl std::ops::Add<Direction> for Coord {
    type Output = Coord;
    fn add(self, dir: Direction) -> Coord {
        self.step(dir)
    }
}
