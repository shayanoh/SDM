/** Window-level state that is not part of the download list. */
export class UiState {
  /** The error line shown in the banner, or null. */
  error = $state<string | null>(null);

  report(message: string) {
    this.error = message;
  }

  dismiss() {
    this.error = null;
  }
}
