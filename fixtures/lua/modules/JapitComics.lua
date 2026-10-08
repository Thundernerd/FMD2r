----------------------------------------------------------------------------------------------------
-- Local Constants
----------------------------------------------------------------------------------------------------

local Template = require 'templates.MangaReaderOnline'
-- DirectoryParameters = '/'            --> Override template variable by uncommenting this line.
XPathTokenStatus    = 'Статус:'
XPathTokenAuthors   = 'Автор(ы):'
XPathTokenArtists   = 'Жудожник:'
XPathTokenGenres    = 'Жанры:'

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
	m.ID                       = '0e04661ef309440d9c3ebea796f307dd'
	m.Name                     = 'JapitComics'
	m.RootURL                  = 'https://j-comics.ru'
	m.Category                 = 'Russian'
	m.OnGetInfo                = 'GetInfo'
	m.OnGetNameAndLink         = 'GetNameAndLink'
	m.OnGetPageNumber          = 'GetPageNumber'
end