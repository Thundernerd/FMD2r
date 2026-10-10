// @vitest-environment jsdom
import { fireEvent, render, screen } from '@testing-library/svelte';
import { describe, expect, it } from 'vitest';
import { Draft } from '#lib/settings/draft.svelte.ts';
import type { Field } from '#lib/settings/fields.ts';
import SettingField from './SettingField.svelte';

const PASSWORD: Field = {
	path: 'connections.proxy.password',
	label: 'Proxy password',
	control: { kind: 'secret' }
};
const HOST: Field = {
	path: 'connections.proxy.host',
	label: 'Proxy host',
	control: { kind: 'text' }
};

/** The settings as the server sends them: a secret only as its `has_` flag. */
const saved = (hasPassword: boolean) => ({
	connections: { proxy: { host: '', username: 'me', has_password: hasPassword } }
});

function renderFields(hasPassword: boolean) {
	const draft = new Draft<object>(saved(hasPassword));
	render(SettingField, { field: PASSWORD, draft });
	render(SettingField, { field: HOST, draft });
	return draft;
}

describe('a secret setting', () => {
	it('shows whether it is set, never a value', () => {
		renderFields(true);
		expect(screen.getByText('Set')).toBeTruthy();
		expect((screen.getByLabelText('Proxy password') as HTMLInputElement).value).toBe('');
	});

	it('shows when it is not set', () => {
		renderFields(false);
		expect(screen.getByText('Not set')).toBeTruthy();
		expect(screen.queryByRole('button', { name: 'Clear' })).toBeNull();
	});

	it('is not sent when another field is saved', async () => {
		const draft = renderFields(true);
		await fireEvent.input(screen.getByLabelText('Proxy host'), {
			target: { value: 'proxy.example' }
		});
		expect(draft.changes()).toEqual({ connections: { proxy: { host: 'proxy.example' } } });
	});

	it('is sent only when typed, and not once the typing is erased', async () => {
		const draft = renderFields(true);
		const input = screen.getByLabelText('Proxy password');
		await fireEvent.input(input, { target: { value: 'new' } });
		expect(draft.changes()).toEqual({ connections: { proxy: { password: 'new' } } });

		await fireEvent.input(input, { target: { value: '' } });
		expect(draft.changes()).toBeNull();
		expect(draft.isDirty(PASSWORD.path)).toBe(false);
	});

	it('is cleared with an empty value', async () => {
		const draft = renderFields(true);
		await fireEvent.click(screen.getByRole('button', { name: 'Clear' }));
		expect(draft.changes()).toEqual({ connections: { proxy: { password: '' } } });
		expect(screen.getByText('Cleared when saved')).toBeTruthy();
	});
});

const ACCENT: Field = {
	path: 'appearance.accent',
	label: 'Accent colour',
	control: {
		kind: 'swatches',
		choices: [
			{ value: 'teal', label: 'Teal' },
			{ value: 'blue', label: 'Blue' },
			{ value: 'purple', label: 'Purple' }
		]
	}
};

describe('a swatches setting', () => {
	it('shows which swatch is picked and saves the one clicked', async () => {
		const draft = new Draft<object>({ appearance: { accent: 'teal' } });
		render(SettingField, { field: ACCENT, draft });
		const teal = screen.getByRole('radio', { name: 'Teal' });
		const purple = screen.getByRole('radio', { name: 'Purple' });
		expect(teal.getAttribute('aria-checked')).toBe('true');
		expect(purple.getAttribute('aria-checked')).toBe('false');

		await fireEvent.click(purple);
		expect(draft.changes()).toEqual({ appearance: { accent: 'purple' } });
		expect(purple.getAttribute('aria-checked')).toBe('true');
		expect(teal.getAttribute('aria-checked')).toBe('false');
		expect(screen.getByRole('radiogroup', { name: 'Accent colour' })).toBeTruthy();
	});
});
