// Port of apps/purepoint-macos/purepoint-macos/Views/TitlebarClearance.swift (WIN-7.1).
//
// What a detail view's top strip must clear. On the Mac only the traffic lights and the sidebar
// toggle need clearing, and only while the sidebar is hidden (`leading` and `height`). On Windows
// the title bar is a band across the whole window (menu bar, sidebar toggle, caption buttons), so
// every screen starts below it: `height` always applies, and a strip drawn inside the band would
// also have to stop `trailing` px short of the caption buttons. No Windows screen draws in the
// band, so there is no `leading`.
import { createContext, useContext } from 'react';

/** The title bar band, as Windows 11 draws a caption (32 px). */
export const TITLE_BAR_HEIGHT = 32;
/** One Windows 11 caption button. */
export const CAPTION_BUTTON_WIDTH = 46;
/** Minimize, maximize and close. */
export const CAPTION_BUTTONS_WIDTH = CAPTION_BUTTON_WIDTH * 3;

export interface TitlebarClearance {
  /** How far a strip in the band stops short of the trailing edge (the caption buttons). */
  readonly trailing: number;
  /** The title bar's height, for views that start below it. */
  readonly height: number;
}

export const NO_CLEARANCE: TitlebarClearance = { trailing: 0, height: 0 };

const TitlebarClearanceContext = createContext<TitlebarClearance>(NO_CLEARANCE);

/** Set by the window (ContentView); `NO_CLEARANCE` outside it. */
export const TitlebarClearanceProvider = TitlebarClearanceContext.Provider;

export function useTitlebarClearance(): TitlebarClearance {
  return useContext(TitlebarClearanceContext);
}
