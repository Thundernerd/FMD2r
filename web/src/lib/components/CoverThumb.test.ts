// @vitest-environment jsdom
import { fireEvent, render } from '@testing-library/svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { tick } from 'svelte';
import CoverThumb from './CoverThumb.svelte';

/** The observers the component made, so a test can scroll it into and out of view. */
let observers: { callback: IntersectionObserverCallback; target: Element | null }[] = [];

class FakeObserver {
	entry: { callback: IntersectionObserverCallback; target: Element | null };
	constructor(callback: IntersectionObserverCallback) {
		this.entry = { callback, target: null };
		observers.push(this.entry);
	}
	observe(target: Element) {
		this.entry.target = target;
	}
	disconnect() {
		this.entry.target = null;
	}
	unobserve() {}
	takeRecords() {
		return [];
	}
}

async function scroll(visible: boolean) {
	for (const { callback, target } of observers) {
		if (!target) continue;
		const entry = { isIntersecting: visible, target } as unknown as IntersectionObserverEntry;
		callback([entry], {} as IntersectionObserver);
	}
	await tick();
}

const SRC = '/api/covers/series?module=site&link=%2Fa';

beforeEach(() => {
	observers = [];
	vi.stubGlobal('IntersectionObserver', FakeObserver);
});
afterEach(() => vi.unstubAllGlobals());

describe('CoverThumb', () => {
	it('shows the placeholder, then the image once it loads', async () => {
		const { container } = render(CoverThumb, { src: SRC, title: 'Alpha' });
		const placeholder = container.querySelector('.cover.blank');
		expect(placeholder?.textContent).toBe('Alpha');
		// Nothing is asked for off screen.
		expect(container.querySelector('img')).toBeNull();

		await scroll(true);
		const img = container.querySelector('img');
		expect(img?.getAttribute('src')).toBe(`${SRC}&w=300`);
		expect(container.querySelector('.cover.blank')).not.toBeNull();

		await fireEvent.load(img as HTMLImageElement);
		expect(container.querySelector('.cover.blank')).toBeNull();
		expect(container.querySelector('img')?.classList.contains('loaded')).toBe(true);
	});

	it('keeps the placeholder when the image fails', async () => {
		const { container } = render(CoverThumb, { src: SRC, title: 'Alpha' });
		await scroll(true);
		await fireEvent.error(container.querySelector('img') as HTMLImageElement);

		expect(container.querySelector('img')).toBeNull();
		expect(container.querySelector('.cover.blank')?.textContent).toBe('Alpha');
		// Scrolling back does not ask again.
		await scroll(false);
		await scroll(true);
		expect(container.querySelector('img')).toBeNull();
	});

	it('drops a request that has not loaded when the card scrolls away', async () => {
		const { container } = render(CoverThumb, { src: SRC, title: 'Alpha' });
		await scroll(true);
		expect(container.querySelector('img')).not.toBeNull();

		await scroll(false);
		expect(container.querySelector('img')).toBeNull();
		await scroll(true);
		expect(container.querySelector('img')).not.toBeNull();
	});

	it('asks for nothing without a source', async () => {
		const { container } = render(CoverThumb, { src: null, title: 'Alpha' });
		await scroll(true);
		expect(container.querySelector('img')).toBeNull();
		expect(container.querySelector('.cover.blank')?.textContent).toBe('Alpha');
	});
});

describe('CoverThumb without IntersectionObserver', () => {
	it('keeps the placeholder and asks for nothing', () => {
		vi.stubGlobal('IntersectionObserver', undefined);
		const { container } = render(CoverThumb, { src: SRC, title: 'Alpha' });
		expect(container.querySelector('img')).toBeNull();
		expect(container.querySelector('.cover.blank')?.textContent).toBe('Alpha');
	});
});
