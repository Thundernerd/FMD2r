----------------------------------------------------------------------------------------------------
-- Local Constants
----------------------------------------------------------------------------------------------------

local Template = require 'templates.MangaReaderOnline'
-- DirectoryParameters = '/'            --> Override template variable by uncommenting this line.
XPathTokenStatus    = 'Statut'
-- XPathTokenAuthors   = 'Author(s)'    --> Override template variable by uncommenting this line.
-- XPathTokenArtists   = 'Artist(s)'    --> Override template variable by uncommenting this line.
-- XPathTokenGenres    = 'Categories'   --> Override template variable by uncommenting this line.

----------------------------------------------------------------------------------------------------
-- Event Functions
----------------------------------------------------------------------------------------------------

-- Get info and chapter list for current manga.
function GetInfo()
	local v, x = nil
	local u = MaybeFillHost(MODULE.RootURL, URL)

	if not HTTP.GET(u) then return net_problem end

	x = CreateTXQuery(HTTP.Document)
	MANGAINFO.Title     = x.XPathString('(//div[contains(@class, "container")]//h2)[1]')
	MANGAINFO.CoverLink = x.XPathString('//div[@class="boxed"]/img/@src')
	MANGAINFO.Status    = MangaInfoStatusIfPos(x.XPathString('//dt[text()="' .. XPathTokenStatus .. '"]/following-sibling::dd[1]/span'), 'En cours', 'Complete')
	MANGAINFO.Authors   = x.XPathStringAll('//dt[text()="' .. XPathTokenAuthors .. '"]/following-sibling::dd[1]/a')
	MANGAINFO.Artists   = x.XPathStringAll('//dt[text()="' .. XPathTokenArtists .. '"]/following-sibling::dd[1]/a')
	MANGAINFO.Genres    = x.XPathStringAll('//dt[text()="' .. XPathTokenGenres .. '"]/following-sibling::dd[1]/a')
	MANGAINFO.Summary   = x.XPathString('//div[contains(@class, "well")]/p')
	
	for v in x.XPath('//ul[@class="chapters"]/li/h5').Get() do
		if x.XPathString('normalize-space(.)', v):find('RAW') then
			if MODULE.GetOption('luaincluderaw') then
				MANGAINFO.ChapterLinks.Add(x.XPathString('a/@href', v))
				MANGAINFO.ChapterNames.Add(x.XPathString('normalize-space(.)', v))
			end
		else
			MANGAINFO.ChapterLinks.Add(x.XPathString('a/@href', v))
			MANGAINFO.ChapterNames.Add(x.XPathString('normalize-space(.)', v))
		end
	end
	MANGAINFO.ChapterLinks.Reverse(); MANGAINFO.ChapterNames.Reverse()

	return no_error
end

-- Get LINKS and NAMES from the manga list of the current website.
function GetNameAndLink()
	Template.GetNameAndLink()

	return no_error
end

-- Get the page count for the current chapter.
function GetPageNumber()
	Template.GetPageNumber()

	return no_error
end

----------------------------------------------------------------------------------------------------
-- Module Initialization
----------------------------------------------------------------------------------------------------

function Init()
	local m = NewWebsiteModule()
	m.ID                       = '9d37501925e94fa8a447c0aa34914db6'
	m.Name                     = 'ScanOP'
	m.RootURL                  = 'https://scan-op.cc'
	m.Category                 = 'French'
	m.OnGetInfo                = 'GetInfo'
	m.OnGetNameAndLink         = 'GetNameAndLink'
	m.OnGetPageNumber          = 'GetPageNumber'
	
	local fmd = require 'fmd.env'
	local slang = fmd.SelectedLanguage
	local lang = {
		['en'] = {
			['includeraw'] = 'Show [RAW] chapters'
		},
		['id_ID'] = {
			['includeraw'] = 'Tampilkan bab [RAW]'
		},
		get =
			function(self, key)
				local sel = self[slang]
				if sel == nil then sel = self['en'] end
				return sel[key]
			end
	}
	m.AddOptionCheckBox('luaincluderaw', lang:get('includeraw'), false)
end