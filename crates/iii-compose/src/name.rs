// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! Names for a daemon that did not choose one.
//!
//! A generated namespace is not decoration: it is the address an operator types
//! to reach this daemon, and it is printed for them to copy. A uuid is unique
//! and unreadable, and nobody retypes one — they scroll back for it, or they
//! restart the daemon and lose the projects the old name held.
//!
//! Two words are read once and remembered, which is the whole point. They also
//! collide, where a uuid does not, so this checks the two places a collision
//! would land: the state directory on this machine, and the lease the engine
//! grants. The first is checked here; the second the engine refuses outright,
//! which is loud rather than silent.

/// Deliberately plain: a name is read aloud across a desk and typed from
/// memory, so nothing here is clever, long, or easy to mis-spell.
const ADJECTIVES: &[&str] = &[
    "amber", "ancient", "autumn", "bold", "brave", "brisk", "calm", "civic", "clever", "cobalt",
    "cosmic", "crimson", "curious", "daring", "dawn", "deep", "dusty", "eager", "early", "east",
    "electric", "emerald", "fair", "fearless", "fine", "firm", "fleet", "fluent", "frosty",
    "gentle", "gilded", "glad", "golden", "grand", "green", "happy", "hidden", "humble", "icy",
    "indigo", "ivory", "jolly", "keen", "kind", "late", "lively", "lucky", "lunar", "misty",
    "modest", "noble", "north", "olive", "open", "patient", "plain", "polar", "proud", "quick",
    "quiet", "rapid", "ready", "royal", "ruby", "rustic", "sage", "scarlet", "sharp", "silent",
    "silver", "simple", "smooth", "snowy", "solar", "solid", "south", "spry", "steady", "still",
    "stout", "sunny", "swift", "tidy", "true", "twin", "upper", "urban", "velvet", "vivid", "warm",
    "west", "wild", "winter", "wise", "witty", "young", "zesty",
];

/// Concrete things, for the same reason: a noun that can be pictured is a noun
/// that is remembered.
const NOUNS: &[&str] = &[
    "acorn", "anchor", "arbor", "arrow", "aspen", "badger", "basin", "beacon", "birch", "bison",
    "bloom", "boulder", "branch", "bridge", "brook", "canyon", "cedar", "cliff", "clover", "comet",
    "coral", "cove", "crane", "creek", "crest", "delta", "dune", "eagle", "ember", "falcon",
    "fern", "field", "finch", "fjord", "forest", "fossil", "garden", "geyser", "glacier", "glade",
    "grotto", "harbor", "harvest", "heron", "hollow", "isle", "juniper", "lagoon", "lantern",
    "ledge", "lichen", "lilac", "lupine", "manor", "maple", "marsh", "meadow", "mesa", "mirror",
    "moss", "mountain", "nectar", "oasis", "orchard", "osprey", "otter", "palm", "pebble", "pine",
    "prairie", "quarry", "quill", "rapids", "raven", "reef", "ridge", "river", "sable", "sequoia",
    "shale", "shore", "signal", "spring", "spruce", "summit", "thicket", "thistle", "tundra",
    "valley", "vale", "willow", "wren",
];

/// A readable namespace: `adjective-noun`, and a suffix only if it has to.
///
/// The words are drawn from a v4 uuid's random bytes rather than a new
/// dependency: the entropy is already there and already good enough to pick two
/// list indices.
///
/// `taken` answers whether a name is already spoken for on this machine. A name
/// whose state directory exists belongs to a daemon that ran before, and taking
/// it would mean adopting whatever that daemon left behind. After a few
/// attempts the name carries a short suffix, because an operator waiting on a
/// daemon is better served by an uglier name than by a loop.
pub fn generate(taken: impl Fn(&str) -> bool) -> String {
    for attempt in 0..8 {
        let candidate = draw();
        if !taken(&candidate) {
            return candidate;
        }
        // The lists are far from exhausted; this is the tail, so widen rather
        // than keep drawing from the same pair.
        if attempt >= 4 {
            let wide = format!("{candidate}-{}", suffix());
            if !taken(&wide) {
                return wide;
            }
        }
    }
    format!("{}-{}", draw(), suffix())
}

fn draw() -> String {
    let bytes = *uuid::Uuid::new_v4().as_bytes();
    // Two bytes per index, so a list longer than 256 still gets full reach.
    let adjective = index(bytes[0], bytes[1], ADJECTIVES.len());
    let noun = index(bytes[2], bytes[3], NOUNS.len());
    format!("{}-{}", ADJECTIVES[adjective], NOUNS[noun])
}

fn index(high: u8, low: u8, len: usize) -> usize {
    (u16::from(high) << 8 | u16::from(low)) as usize % len
}

/// Four hex characters: enough to separate two daemons that drew the same
/// words, short enough to still be read out.
fn suffix() -> String {
    let bytes = *uuid::Uuid::new_v4().as_bytes();
    format!("{:02x}{:02x}", bytes[0], bytes[1])
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn a_generated_name_is_two_words() {
        let name = generate(|_| false);
        let parts: Vec<&str> = name.split('-').collect();
        assert_eq!(parts.len(), 2, "expected adjective-noun, got {name}");
        assert!(ADJECTIVES.contains(&parts[0]), "{name}");
        assert!(NOUNS.contains(&parts[1]), "{name}");
    }

    /// The namespace is also a directory and a routing key, so it is held to
    /// the same charset as one an operator writes by hand.
    #[test]
    fn every_pairing_is_a_valid_namespace() {
        for adjective in ADJECTIVES {
            for noun in NOUNS {
                let name = format!("{adjective}-{noun}");
                assert!(
                    crate::namespace::check(&name).is_ok(),
                    "{name} is not a valid namespace"
                );
            }
        }
    }

    #[test]
    fn a_taken_name_is_not_returned() {
        let first = generate(|_| false);
        let again = generate(|candidate| candidate == first);
        assert_ne!(again, first);
    }

    /// The fallback still has to be usable: a machine where everything is taken
    /// gets a suffixed name, not a panic and not a loop.
    #[test]
    fn a_machine_where_everything_is_taken_still_gets_a_name() {
        let name = generate(|_| true);
        assert!(crate::namespace::check(&name).is_ok(), "{name}");
        assert!(name.split('-').count() >= 3, "expected a suffix: {name}");
    }

    /// Not a uniqueness proof — two words collide, which is why `generate`
    /// takes `taken`. This only catches a draw that is stuck.
    #[test]
    fn the_draw_moves() {
        let names: HashSet<String> = (0..64).map(|_| generate(|_| false)).collect();
        assert!(
            names.len() > 32,
            "the draw barely moves: {} of 64",
            names.len()
        );
    }
}
