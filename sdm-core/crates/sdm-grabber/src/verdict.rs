/// A pure function's output over a probe result. Spec §7.3: four outcomes,
/// because each implies a different user action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    Online,
    Offline,
    Faulty { reason: String },
    CheckFailed,
}
