/** A file that starts like a zip, as the mock backend checks: its stand-in FMD2 userdata. */
export const USERDATA_ZIP = {
	name: 'userdata.zip',
	mimeType: 'application/zip',
	buffer: Buffer.from('PK\u0003\u0004 userdata')
};
