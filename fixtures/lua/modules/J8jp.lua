----------------------------------------------------------------------------------------------------
-- Module Initialization
----------------------------------------------------------------------------------------------------

function Init()
	local m = NewWebsiteModule()
	m.ID                       = '452f504992704f11add27f30f47e388a'
	m.Name                     = 'J8jp'
	m.RootURL                  = 'https://j8jp.com'
	m.Category                 = 'Raw'
	m.OnGetNameAndLink         = 'GetNameAndLink'
	m.OnGetInfo                = 'GetInfo'
	m.OnGetPageNumber          = 'GetPageNumber'
	m.OnBeforeDownloadImage    = 'BeforeDownloadImage'
end

----------------------------------------------------------------------------------------------------
-- Local Constants
----------------------------------------------------------------------------------------------------

local Template = require 'templates.MangaThemesia'
-- DirectoryPagination = '/manga/list-mode/'
-- XPathTokenAuthors   = 'Author'
-- XPathTokenArtists   = 'Artist'

----------------------------------------------------------------------------------------------------
-- Event Functions
----------------------------------------------------------------------------------------------------

-- Get links and names from the manga list of the current website.
function GetNameAndLink()
	Template.GetNameAndLink()

	return no_error
end

-- Get info and chapter list for current manga.
function GetInfo()
	Template.GetInfo()

	x = CreateTXQuery(HTTP.Document)
	MANGAINFO.CoverLink = x.XPathString('//div[@itemprop="image"]//img/@data-src'):gsub('^//', 'https://')
	HTTP.Reset()
	HTTP.Headers.Values['Referer'] = MODULE.RootURL

	return no_error
end

-- Get the page count for the current chapter.
function GetPageNumber()
	Template.GetPageNumber()

	return no_error
end

-- Prepare the URL, http header and/or http cookies before downloading an image.
function BeforeDownloadImage()
	HTTP.Headers.Values['Referer'] = MODULE.RootURL

	return true
end