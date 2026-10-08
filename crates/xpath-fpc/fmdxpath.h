/*
 * libfmdxpath: FMD2's XPath/XQuery engine (internettools, configured as in FMD2's
 * baseunits/XQueryEngineHTML.pas) over a plain C ABI.
 *
 * Conventions
 * - Every function is cdecl and never lets a Pascal exception escape. A failure is recorded per thread and read
 *   with fx_last_error().
 * - Strings going in are UTF-8 byte buffers with an explicit length (no NUL terminator needed). A NULL pointer is
 *   only allowed with length 0.
 * - Strings coming out are fx_string values owned by the caller and freed with fx_string_free(). An empty string
 *   may have ptr == NULL.
 * - Every handle has a free function. A value keeps its document alive, so documents and values can be freed in
 *   any order.
 * - Functions that return an fx_value* never return NULL (barring out-of-memory): a failure yields an empty value.
 *
 * Threads
 * - Any thread may call in. A document and the values evaluated from it must not be used from two threads at once;
 *   different documents may.
 * - Each call runs with FPC's default float environment, as FMD2's threads do (overflow, divide-by-zero and invalid
 *   operation raise; `1e308 * 10` is an empty value, not INF), and restores the caller's on return. To make that
 *   possible the library installs a process-wide SIGFPE handler when it loads; it forwards every SIGFPE raised
 *   outside a call to the handler that was installed before.
 * - Call fx_thread_exit() on a thread that used the library before it exits, or its per-thread state leaks.
 */
#ifndef FMDXPATH_H
#define FMDXPATH_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/* A parsed HTML document and its query engine. */
typedef struct fx_doc fx_doc;

/* An XPath/XQuery value (IXQValue): a sequence, node, string, number, JSON object, ... */
typedef struct fx_value fx_value;

/* A UTF-8 byte string owned by the caller. */
typedef struct fx_string {
    const char *ptr;
    size_t len;
} fx_string;

/* The primary type of a value (IXQValue.kind). */
typedef enum fx_kind {
    FX_KIND_UNDEFINED = 0, /* the empty sequence */
    FX_KIND_BOOLEAN = 1,
    FX_KIND_INT64 = 2,
    FX_KIND_NULL = 3, /* JSON null */
    FX_KIND_NODE = 4,
    FX_KIND_SEQUENCE = 5,
    FX_KIND_ARRAY = 6, /* JSON array */
    FX_KIND_DOUBLE = 7,
    FX_KIND_STRING = 8,
    FX_KIND_DECIMAL = 9,
    FX_KIND_BINARY = 10,
    FX_KIND_QNAME = 11,
    FX_KIND_DATE_TIME = 12,
    FX_KIND_OBJECT = 13, /* JSON object */
    FX_KIND_FUNCTION = 14
} fx_kind;

/* Parses HTML with FMD2's settings: HTML model, missing start and end tags repaired, text not trimmed, no comments
 * or processing instructions, no encoding detection. len == 0 gives an empty document, as FMD2's
 * CreateTXQuery(''). Returns NULL only on failure. */
fx_doc *fx_doc_parse(const char *html, size_t len);
void fx_doc_free(fx_doc *doc);

/* Evaluates an XPath/XQuery expression, or a CSS selector when is_css != 0, against the document or, when
 * context_or_null is set, against that value. An error yields an empty value and sets fx_last_error(); success
 * clears it. Never NULL. */
fx_value *fx_eval(fx_doc *doc, const char *expr, size_t len, fx_value *context_or_null, int is_css);

void fx_value_free(fx_value *v);
/* Number of items; 0 for the empty sequence. */
int64_t fx_value_count(fx_value *v);
/* The i-th item, 1-based. Out of range gives an empty value. Never NULL. */
fx_value *fx_value_get(fx_value *v, int64_t i);
/* An fx_kind. */
int fx_value_kind(fx_value *v);
/* IXQValue.toString: for nodes the text content, trimmed (internettools' XQGlobalTrimNodes). */
fx_string fx_value_to_string(fx_value *v);
/* Node serializations. On a non-node: empty, with fx_last_error() set. */
fx_string fx_value_inner_html(fx_value *v);
fx_string fx_value_outer_html(fx_value *v);
fx_string fx_value_inner_text(fx_value *v);
/* An attribute of a node; empty when missing. On a non-node: empty, with fx_last_error() set. */
fx_string fx_value_get_attribute(fx_value *v, const char *name, size_t len);
/* A JSON object property; an empty value when missing or when v is not an object. Never NULL. */
fx_value *fx_value_get_property(fx_value *v, const char *name, size_t len);

void fx_string_free(fx_string s);

/* The message of the calling thread's last failure; empty if none since the last successful fx_eval. */
fx_string fx_last_error(void);

/* Frees the calling thread's per-thread state (internettools' caches, the last error). Call it before the thread
 * exits. Handles stay valid, and the thread may call in again (and should then call this again before exiting). */
void fx_thread_exit(void);

#ifdef __cplusplus
}
#endif

#endif /* FMDXPATH_H */
