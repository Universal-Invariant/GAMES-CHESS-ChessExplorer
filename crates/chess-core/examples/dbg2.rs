use chess_core::board::*;
fn main(){
    let s = State::start();
    let gm = s.generate_moves();
    println!("generated={}", gm.len());
    for m in &gm { println!("gen {} {}", sq_name(m.from), sq_name(m.to)); }
    // test attacked: is a3 (sq(0,5)) attacked by black? pawn on b7? no. check e2 pawn attacks d3/f3
    println!("attacked f3 by black: {}", s.attacked(parse_sq("f3").unwrap(), 1));
    println!("attacked d3 by black: {}", s.attacked(parse_sq("d3").unwrap(), 1));
    let m = Move{ from: parse_sq("e2").unwrap(), to: parse_sq("e3").unwrap(), promo:0, flag:0 };
    let ns = s.make(m);
    println!("after e3 ep={} side={}", ns.ep, ns.side);
}
