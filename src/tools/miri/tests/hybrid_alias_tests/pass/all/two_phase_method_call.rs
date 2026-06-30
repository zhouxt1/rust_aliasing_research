#![no_std]
#![no_main]

use core::panic::PanicInfo;

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}

// testing two-phase borrows


struct Scoreboard {
    score: u32,
}

impl Scoreboard {
    // Requires a mutable borrow
    fn add_score(&mut self, amount: u32) {
        self.score += amount;
    }

    // Requires a shared borrow
    fn get_current_score(&self) -> u32 {
        self.score
    }
}

pub fn update_game() -> u32 {
    let mut board = Scoreboard { score: 10 };

    board.add_score(board.get_current_score());
    board.score
}


#[no_mangle]
pub fn miri_start(_argc: isize, _argv: *const *const u8) -> isize {

    update_game();

    0
}