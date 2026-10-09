/* FMD2's ExecJS (baseunits/Duktape.pas:77-104) on Duktape 2.3.0, as a C function the Rust side
 * calls. Module files come from `lib_dir`, as loadModuleFile reads them from DukLibDir
 * (baseunits/Duktape.pas:106-121). */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "duktape.h"
#include "duk_module_duktape.h"

/* baseunits/Duktape.pas:22-30: joins the arguments with spaces; the log line is dropped. */
static duk_ret_t native_print(duk_context *ctx) {
	duk_push_string(ctx, " ");
	duk_insert(ctx, 0);
	duk_join(ctx, duk_get_top(ctx) - 1);
	(void) duk_safe_to_string(ctx, -1);
	return 0;
}

/* Reads `path` into a malloc'd buffer, or returns NULL when it is not a readable file. */
static char *read_file(const char *path, size_t *len) {
	FILE *f = fopen(path, "rb");
	char *buf = NULL;
	long size;
	if (f == NULL) return NULL;
	if (fseek(f, 0, SEEK_END) == 0 && (size = ftell(f)) >= 0 && fseek(f, 0, SEEK_SET) == 0) {
		buf = malloc((size_t) size + 1);
		if (buf != NULL && fread(buf, 1, (size_t) size, f) != (size_t) size) {
			free(buf);
			buf = NULL;
		}
		*len = (size_t) size;
	}
	fclose(f);
	return buf;
}

/* baseunits/Duktape.pas:39-67 with loadModuleFile (106-121): `<lib dir>/<id>`, else
 * `<lib dir>/<id>.js`; no file pushes nothing, so require returns the empty exports. */
static duk_ret_t native_mod_search(duk_context *ctx) {
	const char *id = duk_safe_to_string(ctx, 0);
	const char *lib_dir;
	char *path, *source;
	size_t path_len, len = 0;

	duk_push_global_stash(ctx);
	duk_get_prop_string(ctx, -1, "libDir");
	lib_dir = duk_get_string(ctx, -1);
	path_len = strlen(lib_dir) + 1 + strlen(id) + 4;
	path = malloc(path_len);
	if (path == NULL) return 0;
	snprintf(path, path_len, "%s/%s", lib_dir, id);
	source = read_file(path, &len);
	if (source == NULL) {
		snprintf(path, path_len, "%s/%s.js", lib_dir, id);
		source = read_file(path, &len);
	}
	free(path);
	if (source == NULL) return 0;
	duk_push_lstring(ctx, source, len);
	free(source);
	return 1;
}

/* Runs `src` like ExecJS: 0 with the completion value's duk_safe_to_lstring in `*out`, or 1 with
 * the error's. `*out` is malloc'd and released with fmd_duk_free. */
int fmd_duk_exec(const char *src, size_t src_len, const char *lib_dir, char **out, size_t *out_len) {
	duk_context *ctx = duk_create_heap_default();
	const char *result;
	duk_size_t len;
	int rc;

	*out = NULL;
	*out_len = 0;
	if (ctx == NULL) return 2;
	duk_push_global_stash(ctx);
	duk_push_string(ctx, lib_dir);
	duk_put_prop_string(ctx, -2, "libDir");
	duk_pop(ctx);

	duk_module_duktape_init(ctx);
	duk_get_global_string(ctx, "Duktape");
	duk_push_c_function(ctx, native_mod_search, 4);
	duk_put_prop_string(ctx, -2, "modSearch");
	duk_pop(ctx);
	duk_push_c_function(ctx, native_print, DUK_VARARGS);
	duk_put_global_string(ctx, "print");

	duk_push_lstring(ctx, src, src_len);
	rc = duk_peval(ctx) != DUK_EXEC_SUCCESS;
	result = duk_safe_to_lstring(ctx, -1, &len);
	*out = malloc(len + 1);
	if (*out != NULL) {
		memcpy(*out, result, len);
		*out_len = len;
	}
	duk_destroy_heap(ctx);
	return rc;
}

void fmd_duk_free(char *p) {
	free(p);
}
