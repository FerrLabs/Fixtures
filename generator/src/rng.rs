use git2::Time;

pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed)
    }

    fn next_u64(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    pub fn usize(&mut self, max: usize) -> usize {
        (self.next_u64() % max as u64) as usize
    }

    pub fn pick<'a>(&mut self, items: &'a [&str]) -> &'a str {
        items[self.usize(items.len())]
    }
}

pub const COMMIT_TYPES: &[&str] = &[
    "feat", "fix", "refactor", "perf", "chore", "docs", "ci", "test",
];
pub const WORDS_A: &[&str] = &[
    "update",
    "add",
    "remove",
    "refactor",
    "improve",
    "fix",
    "handle",
    "support",
    "implement",
    "optimize",
];
pub const WORDS_B: &[&str] = &[
    "feature",
    "endpoint",
    "handler",
    "logic",
    "validation",
    "error",
    "check",
    "flow",
    "config",
    "output",
];

pub fn rand_message(rng: &mut Rng, scope: &str) -> String {
    let t = rng.pick(COMMIT_TYPES);
    let bang = if rng.usize(20) == 0 { "!" } else { "" };
    let a = rng.pick(WORDS_A);
    let b = rng.pick(WORDS_B);
    format!("{t}({scope}){bang}: {a} {b}")
}

pub fn rand_time(rng: &mut Rng, now: i64) -> Time {
    let days = rng.usize(365) as i64;
    let hours = rng.usize(24) as i64;
    let mins = rng.usize(60) as i64;
    let offset = days * 86400 + hours * 3600 + mins * 60;
    Time::new(now - offset, 0)
}
