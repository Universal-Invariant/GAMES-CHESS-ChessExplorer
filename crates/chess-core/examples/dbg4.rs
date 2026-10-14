use chess_core::board::*;
fn main(){
    let s = State::start();
    println!("generated={}", s.generate_moves().len());
    for m in s.generate_moves() { println!("gen {} {}", sq_name(m.from), sq_name(m.to)); }
}
