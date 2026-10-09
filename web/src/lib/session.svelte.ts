/**
 * Whether the server wants the password, and whether this browser lacks a live session: set
 * when any API call answers 401, cleared by logging in.
 */
export class SessionStore {
	/** The server requires the password (`GET /api/health`), so logging out means something. */
	required = $state(false);
	/** The last API call was refused for want of a session: show the login screen. */
	locked = $state(false);

	/** What `GET /api/health` said about auth. */
	serverRequiresAuth(required: boolean) {
		this.required = required;
	}

	/** An API call answered 401, or this browser logged out. */
	unauthorized() {
		this.required = true;
		this.locked = true;
	}

	loggedIn() {
		this.locked = false;
	}
}
