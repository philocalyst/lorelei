// The host and fixture patch compile this same ABI declaration independently.
macro_rules! demo_state {
    ($level:ty) => {
        #[repr(C)]
        pub struct DemoState {
            pub level: $level,
        }

        pub const STATE_TOKEN: u64 = {
            let field = stringify!($level).as_bytes();
            let mut index = 0;
            let mut tag = 0x91aac0ae1722e831u64;
            while index < field.len() {
                tag = (tag ^ field[index] as u64).wrapping_mul(0x100000001b3);
                index += 1;
            }
            tag ^ ((core::mem::size_of::<DemoState>() as u64) << 32)
                ^ ((core::mem::align_of::<DemoState>() as u64) << 16)
                ^ core::mem::offset_of!(DemoState, level) as u64
        };
    };
}
