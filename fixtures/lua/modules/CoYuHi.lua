----------------------------------------------------------------------------------------------------
-- Local Constants
----------------------------------------------------------------------------------------------------

local Template = require 'templates.MangaReaderOnline'
-- DirectoryParameters = '/'            --> Override template variable by uncommenting this line.
XPathTokenStatus    = 'Estado'
XPathTokenAuthors   = 'Autor(es)'
-- XPathTokenArtists   = 'Artist(s)'    --> Override template variable by uncommenting this line.
XPathTokenGenres    = 'Categorías'

----------------------------------------------------------------------------------------------------
-- Event Functions
----------------------------------------------------------------------------------------------------

-- Get info and chapter list for current manga.
function GetInfo()
	Template.GetInfo()

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
	m.ID                       = '5982635a102f4ff494da287cc335aab7'
	m.Name                     = 'CoYuHi'
	m.RootURL                  = 'http://www.universoyuri.com'
	m.Category                 = 'Spanish-Scanlation'
	m.OnGetInfo                = 'GetInfo'
	m.OnGetNameAndLink         = 'GetNameAndLink'
	m.OnGetPageNumber          = 'GetPageNumber'
end