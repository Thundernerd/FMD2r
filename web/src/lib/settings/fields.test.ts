import { describe, expect, it } from 'vitest';
import type { ModuleOptionSetting } from '#lib/api/types.ts';
import { optionFields } from '#lib/settings/fields.ts';

const OPTIONS: ModuleOptionSetting[] = [
	{ kind: 'checkbox', key: 'hq', caption: 'High quality', default: true, value: false },
	{ kind: 'edit', key: 'lang', caption: 'Language', default: 'en', value: 'de' },
	{ kind: 'spinedit', key: 'delay', caption: 'Delay', default: 3, value: 3, min: 0, max: 10000 },
	{
		kind: 'combobox',
		key: 'server',
		caption: 'Server',
		items: ['One', 'Two'],
		default: 1,
		value: 0
	}
];

describe('form generation from module option definitions', () => {
	it('maps each AddOption* kind onto its control, keyed by options.<key>', () => {
		expect(optionFields(OPTIONS)).toEqual([
			{ path: 'options.hq', label: 'High quality', control: { kind: 'checkbox' } },
			{ path: 'options.lang', label: 'Language', control: { kind: 'text' } },
			{ path: 'options.delay', label: 'Delay', control: { kind: 'number', min: 0, max: 10000 } },
			{
				path: 'options.server',
				label: 'Server',
				control: {
					kind: 'select',
					choices: [
						{ value: 0, label: 'One' },
						{ value: 1, label: 'Two' }
					]
				}
			}
		]);
	});

	it('labels an option without a caption by its key', () => {
		const [field] = optionFields([
			{ kind: 'checkbox', key: 'adult', caption: '', default: false, value: false }
		]);
		expect(field?.label).toBe('adult');
	});

	it('keeps a stored combo index past the items selectable', () => {
		const [field] = optionFields([
			{ kind: 'combobox', key: 'srv', caption: 'Server', items: ['A'], default: 0, value: 3 }
		]);
		expect(field?.control).toEqual({
			kind: 'select',
			choices: [
				{ value: 0, label: 'A' },
				{ value: 3, label: '(item 3)' }
			]
		});
	});
});
