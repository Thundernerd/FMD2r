-- Throwaway: records FMD2's own EncryptString/DecryptString (baseunits/uBaseUnit.pas:1559-1587).
local function hex(s) return (s:gsub('.', function(c) return string.format('%02x', c:byte()) end)) end
function Init()
	local c = require 'fmd.crypto'
	local f = assert(io.open('userdata\\vectors.tsv', 'wb'))
	local plain = {
		'', 'a', 'hunter2', 'fixture-user@example.test', 'not-a-real-password',
		'proxy-user', 'pr0xy p\195\164ss \226\130\172', 'nul\0inside',
		'exactly sixteen!', 'a plaintext that is longer than two AES blocks of sixteen bytes',
	}
	for _, p in ipairs(plain) do
		local e = c.EncryptString(p)
		f:write('enc\t', hex(p), '\t', e, '\t', hex(c.DecryptString(e)), '\n')
	end
	f:close()
end
