/* duktape.c as FMD2's Windows build has it for dates: the Windows config defines no platform
 * date parser or formatter (duk_config.h, "PRS and FMT are intentionally left undefined"), so
 * Date.parse takes ISO 8601 only and toLocale*String print ISO 8601. On Linux the config picks
 * strptime/strftime, so the config is read first, those providers are dropped, and duktape.c
 * then finds the config already included. */
#define DUK_COMPILING_DUKTAPE
#include "duk_config.h"
#undef DUK_USE_DATE_PARSE_STRING
#undef DUK_USE_DATE_FORMAT_STRING
#include "duktape.c"
