// jsdom has no layout: give the scroll views with `className` a size, so a virtualized list
// inside them draws its rows (WIN-8.4's file tree outline).
export function stubScrollViewLayout(className: string, width: number, height: number): void {
  const isScrollView = (element: Element) => element.classList.contains(className);
  // eslint-disable-next-line @typescript-eslint/unbound-method -- called with each element below
  const realRect = Element.prototype.getBoundingClientRect;
  Element.prototype.getBoundingClientRect = function (this: Element) {
    return isScrollView(this) ? new DOMRect(0, 0, width, height) : realRect.call(this);
  };
  for (const [property, size] of [
    ['clientWidth', width],
    ['clientHeight', height],
  ] as const) {
    Object.defineProperty(HTMLElement.prototype, property, {
      configurable: true,
      get(this: HTMLElement) {
        return isScrollView(this) ? size : 0;
      },
    });
  }
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  };
}
