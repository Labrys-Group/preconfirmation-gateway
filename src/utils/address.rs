/// Normalize an Ethereum address for case-insensitive comparison.
///
/// Strips an optional "0x" or "0X" prefix and returns the address in lowercase
/// without the prefix.
///
/// # Examples
///
/// ```
/// use preconfirmation_gateway::utils::address::normalize_address;
///
/// assert_eq!(
///     normalize_address("0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266"),
///     "f39fd6e51aad88f6f4ce6ab8827279cfffb92266"
/// );
///
/// assert_eq!(
///     normalize_address("f39Fd6e51aad88F6F4ce6aB8827279cffFb92266"),
///     "f39fd6e51aad88f6f4ce6ab8827279cfffb92266"
/// );
/// ```
pub fn normalize_address(addr: &str) -> String {
    // Strip 0x or 0X prefix case-insensitively, then convert to lowercase
    let without_prefix = if addr.len() >= 2 && &addr[..2].to_ascii_lowercase() == "0x" {
        &addr[2..]
    } else {
        addr
    };
    without_prefix.to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_with_0x_prefix() {
        let addr = "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266";
        assert_eq!(
            normalize_address(addr),
            "f39fd6e51aad88f6f4ce6ab8827279cfffb92266"
        );
    }

    #[test]
    fn test_normalize_without_0x_prefix() {
        let addr = "f39Fd6e51aad88F6F4ce6aB8827279cffFb92266";
        assert_eq!(
            normalize_address(addr),
            "f39fd6e51aad88f6f4ce6ab8827279cfffb92266"
        );
    }

    #[test]
    fn test_normalize_already_lowercase() {
        let addr = "0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266";
        assert_eq!(
            normalize_address(addr),
            "f39fd6e51aad88f6f4ce6ab8827279cfffb92266"
        );
    }

    #[test]
    fn test_normalize_all_uppercase() {
        let addr = "0XF39FD6E51AAD88F6F4CE6AB8827279CFFFB92266";
        let expected = "f39fd6e51aad88f6f4ce6ab8827279cfffb92266";
        assert_eq!(normalize_address(addr), expected);
    }

    #[test]
    fn test_normalize_comparison() {
        let addr1 = "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266";
        let addr2 = "0XF39FD6E51AAD88F6F4CE6AB8827279CFFFB92266";
        let addr3 = "f39fd6e51aad88f6f4ce6ab8827279cfffb92266";

        let normalized1 = normalize_address(addr1);
        let normalized2 = normalize_address(addr2);
        let normalized3 = normalize_address(addr3);

        assert_eq!(normalized1, normalized2);
        assert_eq!(normalized1, normalized3);
        assert_eq!(normalized2, normalized3);
    }
}