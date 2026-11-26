// Port of apps/purepoint-macos/purepoint-macos/Views/CommandPalette/CommandPaletteRowView.swift
// (WIN-7.5): a 44px palette row with its icon, bold name, secondary subtitle and the category
// badge (Agent, Swarm, File), highlighted with the accent at 15% when selected.
import {
  type CommandPaletteItem,
  categoryLabel,
  displayName,
  itemId,
  subtitle,
} from '../../model/commandPalette.ts';
import { Icon } from '../../theme/Icon.tsx';
import { paletteIcon } from './paletteFilter.ts';

export interface CommandPaletteRowViewProps {
  item: CommandPaletteItem;
  isHighlighted: boolean;
  /** A single click highlights the row (Swift `shouldSelectRow`). */
  onHighlight: () => void;
  /** A double click chooses it (Swift `tableViewDoubleClick`). */
  onChoose: () => void;
  /** The list's id prefix, for `aria-activedescendant`. */
  idPrefix: string;
  index: number;
}

export const paletteRowId = (idPrefix: string, index: number): string =>
  `${idPrefix}-row-${String(index)}`;

export function CommandPaletteRowView({
  item,
  isHighlighted,
  onHighlight,
  onChoose,
  idPrefix,
  index,
}: CommandPaletteRowViewProps) {
  const category = categoryLabel(item);
  return (
    <div
      id={paletteRowId(idPrefix, index)}
      className="palette-row"
      role="option"
      aria-selected={isHighlighted}
      data-testid="palette-row"
      data-item-id={itemId(item)}
      data-highlighted={isHighlighted}
      // Keep focus in the search field, as the Mac's table never takes it.
      onMouseDown={(event) => {
        event.preventDefault();
        onHighlight();
      }}
      onDoubleClick={onChoose}
    >
      <Icon name={paletteIcon(item)} size={20} className="palette-row-icon" />
      <span className="palette-row-text">
        <span className="palette-row-name">{displayName(item)}</span>
        <span className="palette-row-subtitle">{subtitle(item)}</span>
        {category !== null && <span className="palette-row-badge">{category}</span>}
      </span>
    </div>
  );
}
