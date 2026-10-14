//! Marks key events that Keylens synthesizes, so its own key listener can ignore them.

/// Value stored in `kCGEventSourceUserData` on events we post ("KEYL").
pub const MARK: i64 = 0x4B45_594C;

pub fn is_ours(user_data: i64) -> bool {
    user_data == MARK
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_our_mark_is_recognized() {
        assert!(is_ours(MARK));
        assert!(!is_ours(0));
        assert!(!is_ours(MARK + 1));
    }
}
