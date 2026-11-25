import type { Locator, Page } from '@playwright/test';

/** The inspector pane. */
export const pane = (page: Page): Locator => page.getByTestId('auto-side-panel').first();
