use chess_core::board::*;
fn main() {
    let s = State::from_fen("r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1").unwrap();
    let m = s.legal_moves().into_iter().find(|m| sq_name(m.from)=="e1" && sq_name(m.to)=="g1").unwrap();
    let ns = s.make(m);
    println!("after O-O: {}", ns.to_fen());
    for lm in ns.legal_moves() {
        println!("  black reply {}->{}", sq_name(lm.from), sq_name(lm.to));
    }
}
