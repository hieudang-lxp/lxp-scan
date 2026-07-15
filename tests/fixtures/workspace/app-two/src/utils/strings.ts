// verbatim copy of app-one's collapseWhitespace under a different local name
function collapseWhitespace(text: string) {
  return text.replace(/\s+/g, ' ').trim()
}

export const tidyTitle = (title: string) => collapseWhitespace(title)
