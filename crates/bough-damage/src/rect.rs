//! Half-open device-pixel rectangles.
/// A half-open rectangle in device pixels.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Hash)]
pub struct Rect {
    pub x0: i32,
    pub y0: i32,
    pub x1: i32,
    pub y1: i32,
}

impl Rect {
    pub const EMPTY: Rect = Rect { x0: 0, y0: 0, x1: 0, y1: 0 };
    /// Larger than any window, small enough that shifting it cannot overflow.
    pub const EVERYWHERE: Rect = Rect { x0: -(1 << 28), y0: -(1 << 28), x1: 1 << 28, y1: 1 << 28 };
    #[inline]
    pub const fn new(x0: i32, y0: i32, x1: i32, y1: i32) -> Rect {
        Rect { x0, y0, x1, y1 }
    }
    #[inline]
    pub const fn xywh(x: i32, y: i32, w: i32, h: i32) -> Rect {
        Rect { x0: x, y0: y, x1: x + w, y1: y + h }
    }
    #[inline]
    pub const fn is_empty(&self) -> bool {
        self.x1 <= self.x0 || self.y1 <= self.y0
    }
    #[inline]
    pub fn and(self, o: Rect) -> Rect {
        let r = Rect { x0: self.x0.max(o.x0), y0: self.y0.max(o.y0), x1: self.x1.min(o.x1), y1: self.y1.min(o.y1) };
        if r.is_empty() { Rect::EMPTY } else { r }
    }
    /// The bounding box of both (an empty rect is the identity).
    #[inline]
    pub fn hull(self, o: Rect) -> Rect {
        if self.is_empty() {
            return o;
        }
        if o.is_empty() {
            return self;
        }
        Rect { x0: self.x0.min(o.x0), y0: self.y0.min(o.y0), x1: self.x1.max(o.x1), y1: self.y1.max(o.y1) }
    }
    #[inline]
    pub fn hits(self, o: Rect) -> bool {
        self.x0 < o.x1 && o.x0 < self.x1 && self.y0 < o.y1 && o.y0 < self.y1 && !self.is_empty() && !o.is_empty()
    }
    #[inline]
    pub fn contains(self, o: Rect) -> bool {
        o.is_empty() || (self.x0 <= o.x0 && self.y0 <= o.y0 && self.x1 >= o.x1 && self.y1 >= o.y1)
    }
    #[inline]
    pub fn has(self, x: i32, y: i32) -> bool {
        x >= self.x0 && x < self.x1 && y >= self.y0 && y < self.y1
    }
    #[inline]
    pub fn shift(self, dx: i32, dy: i32) -> Rect {
        if self.is_empty() {
            return Rect::EMPTY;
        }
        Rect { x0: self.x0 + dx, y0: self.y0 + dy, x1: self.x1 + dx, y1: self.y1 + dy }
    }
    #[inline]
    pub fn expand(self, n: i32) -> Rect {
        self.expand4(n, n, n, n)
    }
    #[inline]
    pub fn expand4(self, l: i32, t: i32, r: i32, b: i32) -> Rect {
        if self.is_empty() {
            return Rect::EMPTY;
        }
        Rect { x0: self.x0 - l, y0: self.y0 - t, x1: self.x1 + r, y1: self.y1 + b }
    }
    #[inline]
    pub fn area(self) -> i64 {
        if self.is_empty() { 0 } else { (self.x1 - self.x0) as i64 * (self.y1 - self.y0) as i64 }
    }
}
