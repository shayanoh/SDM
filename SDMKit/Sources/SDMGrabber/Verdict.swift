/// A pure function's output over a probe result. Spec §7.3: four outcomes,
/// because each implies a different user action.
public enum Verdict: Equatable, Sendable {
    case online
    case offline
    case faulty(reason: String)
    case checkFailed
}
