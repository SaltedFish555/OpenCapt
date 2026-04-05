#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SelectionRect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

impl SelectionRect {
    pub fn from_coords(start_x: i32, start_y: i32, end_x: i32, end_y: i32) -> Option<Self> {
        let left = start_x.min(end_x).max(0);
        let top = start_y.min(end_y).max(0);
        let right = start_x.max(end_x).max(0);
        let bottom = start_y.max(end_y).max(0);
        let width = (right - left) as u32;
        let height = (bottom - top) as u32;
        if width == 0 || height == 0 {
            None
        } else {
            Some(Self {
                x: left,
                y: top,
                width,
                height,
            })
        }
    }

    #[cfg(test)]
    pub fn contains(self, x: i32, y: i32) -> bool {
        x >= self.x
            && x < self.x + self.width as i32
            && y >= self.y
            && y < self.y + self.height as i32
    }
}
