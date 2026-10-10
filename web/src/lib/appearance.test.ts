// @vitest-environment jsdom
import { beforeEach, describe, expect, it } from 'vitest';
import { applyAppearance } from '#lib/appearance.ts';

const html = () => document.documentElement;

describe('applying an appearance', () => {
	beforeEach(() => {
		html().removeAttribute('data-theme');
		html().removeAttribute('data-accent');
		html().style.removeProperty('--text-scale');
	});

	it('sets the theme, the accent and the text scale on <html>', () => {
		applyAppearance({ mode: 'dark', text_size: 'large', accent: 'purple' });
		expect(html().dataset['theme']).toBe('dark');
		expect(html().dataset['accent']).toBe('purple');
		expect(html().style.getPropertyValue('--text-scale')).toBe('1.15');

		applyAppearance({ mode: 'light', text_size: 'larger', accent: 'blue' });
		expect(html().dataset['theme']).toBe('light');
		expect(html().dataset['accent']).toBe('blue');
		expect(html().style.getPropertyValue('--text-scale')).toBe('1.3');
	});

	it('leaves the theme to the device in system mode', () => {
		applyAppearance({ mode: 'dark', text_size: 'small', accent: 'red' });
		applyAppearance({ mode: 'system', text_size: 'normal', accent: 'teal' });
		expect(html().hasAttribute('data-theme')).toBe(false);
		expect(html().dataset['accent']).toBe('teal');
		expect(html().style.getPropertyValue('--text-scale')).toBe('1');

		applyAppearance({ mode: 'system', text_size: 'small', accent: 'teal' });
		expect(html().style.getPropertyValue('--text-scale')).toBe('0.9');
	});
});
