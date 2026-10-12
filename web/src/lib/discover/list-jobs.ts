import type { ListFailureReason, ListJobKind, ModuleSummary } from '#lib/api/types.ts';

/** What a failed list job of `module` says, by why it failed; the server words the Jobs panel's
 * "Last error" the same (`crates/fmd-core/src/lists/jobs.rs`). */
export function listFailedText(
	module: Pick<ModuleSummary, 'name' | 'capabilities'>,
	reason: ListFailureReason | null | undefined,
	job: ListJobKind
): string {
	const site = module.name;
	switch (reason) {
		case 'no_dump':
			return module.capabilities.update_list
				? `There is no ready-made list for ${site} yet. Use Update list to build it from the website.`
				: `There is no ready-made list for ${site} yet, and this website cannot build one itself.`;
		case 'unreachable':
			return `Could not reach the ready-made lists to get the list of ${site}. Check the connection and try again later.`;
		case 'bad_archive':
			return `The ready-made list of ${site} is damaged or empty.`;
		default:
			return job === 'update'
				? `Updating the list of ${site} failed.`
				: `Getting the ready-made list of ${site} failed.`;
	}
}
