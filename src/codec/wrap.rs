//! `i32` whose arithmetic wraps, as Go's does. Cargo ignores a dependency's `[profile]` tables, so a
//! consumer's dev build has overflow checks on; code that relies on wrap-around says so here.

use std::ops::{Add, AddAssign, Mul, Neg, Shr, Sub, SubAssign};

#[derive(Clone, Copy)]
pub(super) struct W(pub(super) i32);

macro_rules! wrap_op {
    ($tr:ident, $f:ident, $m:ident) => {
        impl $tr<W> for W {
            type Output = W;
            #[inline(always)]
            fn $f(self, r: W) -> W {
                W(self.0.$m(r.0))
            }
        }
        impl $tr<i32> for W {
            type Output = W;
            #[inline(always)]
            fn $f(self, r: i32) -> W {
                W(self.0.$m(r))
            }
        }
    };
}

wrap_op!(Add, add, wrapping_add);
wrap_op!(Sub, sub, wrapping_sub);
wrap_op!(Mul, mul, wrapping_mul);

impl AddAssign<W> for W {
    #[inline(always)]
    fn add_assign(&mut self, r: W) {
        *self = *self + r;
    }
}

impl SubAssign<W> for W {
    #[inline(always)]
    fn sub_assign(&mut self, r: W) {
        *self = *self - r;
    }
}

impl Neg for W {
    type Output = W;
    #[inline(always)]
    fn neg(self) -> W {
        W(self.0.wrapping_neg())
    }
}

impl Shr<u32> for W {
    type Output = W;
    #[inline(always)]
    fn shr(self, n: u32) -> W {
        W(self.0 >> (n & 31))
    }
}
