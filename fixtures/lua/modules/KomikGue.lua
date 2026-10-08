----------------------------------------------------------------------------------------------------
-- Local Constants
----------------------------------------------------------------------------------------------------

local Template = require 'templates.MangaReaderOnline'
-- DirectoryParameters = '/'            --> Override template variable by uncommenting this line.
-- XPathTokenStatus    = 'Status'       --> Override template variable by uncommenting this line.
-- XPathTokenAuthors   = 'Author(s)'    --> Override template variable by uncommenting this line.
-- XPathTokenArtists   = 'Artist(s)'    --> Override template variable by uncommenting this line.
XPathTokenGenres    = 'Genre'

----------------------------------------------------------------------------------------------------
-- Event Functions
----------------------------------------------------------------------------------------------------

-- Get info and chapter list for current manga.
function GetInfo()
	Template.GetInfo()
	local v, x = nil
	local u = MaybeFillHost(MODULE.RootURL, URL)

	if not HTTP.GET(u) then return net_problem end

	x = CreateTXQuery(HTTP.Document)
	MANGAINFO.Title     = x.XPathString('(//div[contains(@class, "container")]//h1)[1]')
	MANGAINFO.Status    = MangaInfoStatusIfPos(x.XPathString('//dt[text()="' .. XPathTokenStatus .. '"]/following-sibling::dd[1]//span'), 'Ongoing', 'Complete')
	MANGAINFO.Authors   = x.XPathStringAll('//dt[text()="' .. XPathTokenAuthors .. '"]/following-sibling::dd[1]//a')
	MANGAINFO.Artists   = x.XPathString('//dt[text()="' .. XPathTokenArtists .. '"]/following-sibling::dd[1]')
	MANGAINFO.Summary   = x.XPathString('//div[contains(@class, "well")]/div')

	v = x.XPath('//div[@class="chapter-wrapper"]/table//td[@class="chapter"]/a')
	for i = 1, v.Count do
		MANGAINFO.ChapterLinks.Add(v.Get(i).GetAttribute('href'))
		MANGAINFO.ChapterNames.Add(x.XPathString('normalize-space(.)', v.Get(i)))
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
	m.ID               = '024d94e40e07481f9ed6cd0412d401e3'
	m.Name             = 'KomikGue'
	m.RootURL          = 'http://www.komikgue.com'
	m.Category         = 'Indonesian'
	m.OnGetInfo        = 'GetInfo'
	m.OnGetNameAndLink = 'GetNameAndLink'
	m.OnGetPageNumber  = 'GetPageNumber'
end