export interface TerminalLog {
  id: number;
  type: 'info' | 'success' | 'warning' | 'error' | 'system' | 'glitch';
  message: string;
  timestamp: string;
  clickableCommand?: string;
}

export enum KeySequence {
  KONAMI = "ArrowUpArrowUpArrowDownArrowDownArrowLeftArrowRightArrowLeftArrowRightba",
  III = "iii"
}
