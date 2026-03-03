use nanoid::nanoid;
use rand::seq::SliceRandom;
use uuid::Uuid;

const ALPHABET: &[char] = &[
    'a', 'b', 'c', 'd', 'e', 'f', 'g', 'h', 'i', 'j', 'k', 'l', 'm', 'n', 'o', 'p', 'q', 'r',
    's', 't', 'u', 'v', 'w', 'x', 'y', 'z', '0', '1', '2', '3', '4', '5', '6', '7', '8', '9',
];

const ID_LEN: usize = 8;

pub fn worktree_id() -> String {
    format!("wt-{}", nanoid!(ID_LEN, ALPHABET))
}

pub fn agent_id() -> String {
    format!("ag-{}", nanoid!(ID_LEN, ALPHABET))
}

pub fn session_id() -> String {
    Uuid::new_v4().to_string()
}

/// Generate a name for root/project-level agents by mashing up elite PG names.
pub fn root_agent_name() -> String {
    const PG_FIRSTS: &[&str] = &[
        "Magic", "John", "Steve", "Jason", "Stephen", "Chris", "Isiah",
        "Penny", "Rajon", "Damian", "Kyrie", "Allen", "Bob", "Walt",
        "Tiny", "Oscar", "Gary", "Russell", "Tony", "Derrick", "Tim",
        "Mark", "Kevin", "Chauncey", "Baron",
    ];
    const PG_LASTS: &[&str] = &[
        "Johnson", "Stockton", "Nash", "Kidd", "Curry", "Paul", "Thomas",
        "Hardaway", "Rondo", "Lillard", "Irving", "Iverson", "Cousy",
        "Frazier", "Archibald", "Robertson", "Payton", "Westbrook",
        "Parker", "Rose", "Price", "Billups", "Davis",
    ];

    let mut rng = rand::thread_rng();
    let first = PG_FIRSTS.choose(&mut rng).unwrap();
    let last = PG_LASTS.choose(&mut rng).unwrap();
    format!("{first} {last}")
}

/// Generate a name for worktree agents by mashing up All-NBA names across all positions.
pub fn worktree_agent_name() -> String {
    const NBA_FIRSTS: &[&str] = &[
        "LeBron", "Kobe", "Michael", "Shaquille", "Tim", "Kevin", "Dirk",
        "Hakeem", "Charles", "Karl", "Patrick", "Scottie", "Dennis",
        "Clyde", "Dominique", "James", "Vince", "Tracy", "Paul", "Ray",
        "Reggie", "Carmelo", "Dwyane", "Giannis", "Kawhi", "Anthony",
        "Julius", "Larry", "Bill", "Wilt",
    ];
    const NBA_LASTS: &[&str] = &[
        "James", "Bryant", "Jordan", "Duncan", "Garnett", "Nowitzki",
        "Olajuwon", "Barkley", "Malone", "Ewing", "Pippen", "Rodman",
        "Drexler", "Wilkins", "Worthy", "Carter", "McGrady", "Pierce",
        "Allen", "Miller", "Anthony", "Wade", "Durant", "Leonard",
        "Davis", "Erving", "Bird", "Russell", "Chamberlain",
    ];

    let mut rng = rand::thread_rng();
    let first = NBA_FIRSTS.choose(&mut rng).unwrap();
    let last = NBA_LASTS.choose(&mut rng).unwrap();
    format!("{first} {last}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn given_worktree_id_should_start_with_wt_prefix() {
        let id = worktree_id();
        assert!(id.starts_with("wt-"), "expected wt- prefix, got {id}");
    }

    #[test]
    fn given_worktree_id_should_have_correct_length() {
        // wt- (3) + 8 chars = 11
        let id = worktree_id();
        assert_eq!(id.len(), 11, "expected length 11, got {} for {id}", id.len());
    }

    #[test]
    fn given_agent_id_should_start_with_ag_prefix() {
        let id = agent_id();
        assert!(id.starts_with("ag-"), "expected ag- prefix, got {id}");
    }

    #[test]
    fn given_agent_id_should_have_correct_length() {
        let id = agent_id();
        assert_eq!(id.len(), 11);
    }

    #[test]
    fn given_session_id_should_be_valid_uuid() {
        let id = session_id();
        assert!(
            Uuid::parse_str(&id).is_ok(),
            "expected valid UUID, got {id}"
        );
    }

    #[test]
    fn given_ids_should_contain_only_lowercase_alphanumeric() {
        for _ in 0..50 {
            let id = agent_id();
            let suffix = &id[3..]; // skip "ag-"
            assert!(
                suffix.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit()),
                "unexpected chars in {id}"
            );
        }
    }

    #[test]
    fn given_100_ids_should_all_be_unique() {
        let ids: HashSet<String> = (0..100).map(|_| agent_id()).collect();
        assert_eq!(ids.len(), 100);
    }

    #[test]
    fn given_root_agent_name_should_have_first_and_last() {
        let name = root_agent_name();
        let parts: Vec<&str> = name.split_whitespace().collect();
        assert_eq!(parts.len(), 2, "expected 'First Last', got {name}");
        assert!(!parts[0].is_empty());
        assert!(!parts[1].is_empty());
    }

    #[test]
    fn given_worktree_agent_name_should_have_first_and_last() {
        let name = worktree_agent_name();
        let parts: Vec<&str> = name.split_whitespace().collect();
        assert_eq!(parts.len(), 2, "expected 'First Last', got {name}");
        assert!(!parts[0].is_empty());
        assert!(!parts[1].is_empty());
    }
}
