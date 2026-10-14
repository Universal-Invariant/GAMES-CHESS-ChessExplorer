use chess_core::board::*;
fn main(){
    let s = State::start();
    // manual pawn push for a2 (idx 48): one=56 empty, promo_row=56 -> promotions only
    // check e2: idx 52, one=60 empty? board[60]=0x0e king. So blocked!
    for i in [48u8,52] {
        println!("idx {} name {} board[+8]={:02x}", i, sq_name(i), s.board[i as usize + 8]);
    }
}
