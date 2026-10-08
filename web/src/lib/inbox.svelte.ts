/** Whether the inbox popover is open and which item it points at; shared so any page can open it. */
export const inboxUi = $state<{ open: boolean; focus: string | null }>({
	open: false,
	focus: null
});

/** Opens the inbox popover scrolled to item `id`. */
export function showInboxItem(id: string): void {
	inboxUi.open = true;
	inboxUi.focus = id;
}
