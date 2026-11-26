import { createElement } from 'react';
import type { LucideProps } from 'lucide-react';
import { lucideIconFor, symbolFill, type SfSymbolName } from './icons.ts';

export interface IconProps extends Omit<LucideProps, 'ref'> {
  /** The SF Symbol name the Mac app uses, such as `gear` or `xmark.circle.fill`. */
  name: SfSymbolName;
  /** Accessible name; without one the icon is decorative and hidden from assistive technology. */
  label?: string;
}

/** Draws the Lucide icon for an SF Symbol (DEC-13), at 16px in the current text color by default. */
export function Icon({ name, label, size = 16, ...rest }: IconProps) {
  const fill = symbolFill(name);
  const fillProps =
    fill === 'none' ? {} : { fill: 'currentColor', fillOpacity: fill === 'tint' ? 0.25 : 1 };
  const a11y = label === undefined ? { 'aria-hidden': true } : { role: 'img', 'aria-label': label };
  const props = { size, 'data-symbol': name, ...fillProps, ...a11y, ...rest };
  // The components are module-level constants in SF_SYMBOLS, so this never creates one per render.
  return createElement(lucideIconFor(name), props);
}
