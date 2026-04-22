use serde::{Deserialize, Serialize};

/// Task priority level for scheduling.
///
/// Ordered from lowest to highest: Low < Normal < High < Critical.
/// Used by the Scheduler to determine execution order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[derive(Default)]
pub enum Priority {
    Low = 0,
    #[default]
    Normal = 1,
    High = 2,
    Critical = 3,
}


impl std::fmt::Display for Priority {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Priority::Low => write!(f, "Low"),
            Priority::Normal => write!(f, "Normal"),
            Priority::High => write!(f, "High"),
            Priority::Critical => write!(f, "Critical"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ordering_critical_greater_than_low() {
        assert!(Priority::Critical > Priority::High);
        assert!(Priority::High > Priority::Normal);
        assert!(Priority::Normal > Priority::Low);
    }

    #[test]
    fn serde_roundtrip() {
        let priorities = [
            Priority::Low,
            Priority::Normal,
            Priority::High,
            Priority::Critical,
        ];
        for p in &priorities {
            let json = serde_json::to_string(p).unwrap();
            let back: Priority = serde_json::from_str(&json).unwrap();
            assert_eq!(*p, back);
        }
    }

    #[test]
    fn display_all_variants() {
        assert_eq!(format!("{}", Priority::Low), "Low");
        assert_eq!(format!("{}", Priority::Normal), "Normal");
        assert_eq!(format!("{}", Priority::High), "High");
        assert_eq!(format!("{}", Priority::Critical), "Critical");
    }

    #[test]
    fn default_is_normal() {
        assert_eq!(Priority::default(), Priority::Normal);
    }
}
