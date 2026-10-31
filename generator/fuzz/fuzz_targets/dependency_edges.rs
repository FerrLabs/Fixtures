#![no_main]
#![allow(dead_code)]

#[path = "../../src/generate.rs"]
mod generate;
#[path = "../../src/rng.rs"]
mod rng;
#[path = "../../src/tree.rs"]
mod tree;
#[path = "../../src/types.rs"]
mod types;

use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;
use rng::Rng;

#[derive(Arbitrary, Debug)]
struct Input {
    pkg_count: u16,
    seed: u64,
}

fuzz_target!(|input: Input| {
    let Ok(mut rng) = Rng::new(input.seed) else {
        return;
    };
    let pkg_count = usize::from(input.pkg_count % 512).max(2);
    let core_count = (pkg_count / 5).max(1);

    let deps = generate::build_dependency_edges(pkg_count, core_count, &mut rng);

    assert_eq!(deps.len(), pkg_count);
    for (i, edges) in deps.iter().enumerate() {
        if i < core_count {
            assert_eq!(edges.len(), usize::from(i > 0));
            assert!(edges.iter().all(|&d| d < i));
        } else {
            assert!(!edges.is_empty() && edges.len() <= 3);
            assert!(edges.iter().all(|&d| d < core_count));
            assert!(edges.windows(2).all(|w| w[0] < w[1]));
        }
    }

    let mut again = Rng::new(input.seed).unwrap();
    assert_eq!(
        deps,
        generate::build_dependency_edges(pkg_count, core_count, &mut again)
    );
});
