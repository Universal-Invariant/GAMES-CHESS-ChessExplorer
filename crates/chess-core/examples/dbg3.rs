use chess_core::board::*;
fn main(){
    let s = State::start();
    // replicate pawn branch for e2 (index 52)
    let sq_e2: u8 = 52;
    println!("e2 idx={} piece={:02x} type={} color={}", sq_e2, s.board[52], piece_type(s.board[52]), color_of(s.board[52]));
    println!("PAWN const={} KNIGHT={} BISHOP={} ROOK={} QUEEN={} KING={}", PAWN, KNIGHT, BISHOP, ROOK, QUEEN, KING);
    let one = sq_e2 as i8 + 8;
    println!("one={} board[one]={}", one, s.board[one as usize]);
    // count white pieces iterated
    let mut n = 0;
    for i in 0..64u8 {
        let p = s.board[i as usize];
        if p != 0 && color_of(p) == 0 {
            n += 1;
            println!("white at {} type={} ({})", sq_name(i), piece_type(p), if piece_type(p)==PAWN {"pawn"} else {"other"});
        }
    }
    println!("white pieces: {}", n);
}
