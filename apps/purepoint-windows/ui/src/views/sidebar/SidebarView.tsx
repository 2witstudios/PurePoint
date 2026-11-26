// Port of apps/purepoint-macos/purepoint-macos/Views/Sidebar/SidebarView.swift (WIN-7.2).
//
// The sidebar column: the Point Guard header (which doubles as the Dashboard button), the other
// nav items, a divider, the project outline (or "No project open"), and the footer.
//
// Differences from Swift: the header draws the dashboard's symbol rather than the PurePoint logo
// image asset.

import { useStore } from 'zustand';
import {
  SIDEBAR_NAV_ITEMS,
  type SidebarNavItem,
  type SidebarSelection,
  navItemTitle,
  selection as sidebarSelection,
  selectionsEqual,
} from '../../model/sidebarItem.ts';
import type { SfSymbolName } from '../../theme/icons.ts';
import { Icon } from '../../theme/Icon.tsx';
import { useAppState } from '../shell/appContext.ts';
import { SidebarFooter } from './SidebarFooter.tsx';
import { SidebarOutlineView } from './SidebarOutlineView.tsx';
import type {
  SidebarExpansionStoreApi,
  SidebarSelectOptions,
} from './SidebarOutlineViewController.tsx';
import './sidebar.css';

/** The SF Symbols the nav items draw (Swift `SidebarNavItem.icon`; `NAV_ITEM_ICONS` in Lucide). */
const NAV_SYMBOLS: Record<SidebarNavItem, SfSymbolName> = {
  dashboard: 'bubble.left.and.bubble.right',
  agents: 'cpu',
  schedule: 'calendar',
};

export interface SidebarViewProps {
  readonly selection: SidebarSelection | null;
  readonly onSelectionChange: (
    selection: SidebarSelection | null,
    options?: SidebarSelectOptions,
  ) => void;
  readonly expansionStore?: SidebarExpansionStoreApi;
}

export function SidebarView({ selection, onSelectionChange, expansionStore }: SidebarViewProps) {
  const app = useAppState();
  const hasProjects = useStore(app.store, (state) => state.projects.length > 0);
  const hasMachines = useStore(app.store, (state) => state.machines.length > 0);
  const dashboard = sidebarSelection.nav('dashboard');

  return (
    <nav className="sidebar-view" aria-label="Sidebar">
      {/* App branding — doubles as the Dashboard button. */}
      <button
        type="button"
        className="sidebar-brand"
        data-testid="sidebar-nav-dashboard"
        aria-current={selectionsEqual(selection, dashboard) ? 'page' : undefined}
        onClick={() => {
          onSelectionChange(dashboard);
        }}
      >
        <Icon name={NAV_SYMBOLS.dashboard} size={16} />
        <span>Point Guard</span>
      </button>

      {/* Nav items — fixed, non-scrollable. */}
      <div className="sidebar-nav">
        {SIDEBAR_NAV_ITEMS.filter((item) => item !== 'dashboard').map((item) => {
          const row = sidebarSelection.nav(item);
          return (
            <button
              type="button"
              key={item}
              className="sidebar-nav-button"
              data-testid={`sidebar-nav-${item}`}
              aria-current={selectionsEqual(selection, row) ? 'page' : undefined}
              onClick={() => {
                onSelectionChange(row);
              }}
            >
              <Icon name={NAV_SYMBOLS[item]} size={14} />
              <span>{navItemTitle(item)}</span>
            </button>
          );
        })}
      </div>

      <div className="sidebar-divider" role="presentation" />

      {/* Project tree — compact 24px rows. */}
      {hasProjects || hasMachines ? (
        <SidebarOutlineView
          selection={selection}
          onSelectionChange={onSelectionChange}
          {...(expansionStore === undefined ? {} : { expansionStore })}
        />
      ) : (
        <div className="sidebar-empty">No project open</div>
      )}

      <SidebarFooter selection={selection} />
    </nav>
  );
}
