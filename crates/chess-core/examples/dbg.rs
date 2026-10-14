use chess_core::board::*;
fn main(){
    let s = State::start();
    println!("fen={}", s.to_fen());
    for i in 0..64 { print!("{:02x} ", s.board[i]); if i%8==7 { println!(); } }
    println!("sq(0,7)={} sq(4,0)={}", sq(0,7), sq(4,0));
    let ms = s.legal_moves();
    println!("count={}", ms.len());
    for m in &ms { println!("{} {}", sq_name(m.from), sq_name(m.to)); }
}
