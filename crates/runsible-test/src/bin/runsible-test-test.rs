//! TRIPLE SIMS gate for runsible-test. Runs `runsible_test::f30()` 3× via
//! `exopack::triple_sims::f60`; exits 0 only if all 3 runs return 0.

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let ok = exopack::triple_sims::f60(|| async { runsible_test::f30() == 0 }).await;
    std::process::exit(if ok { 0 } else { 1 });
}
