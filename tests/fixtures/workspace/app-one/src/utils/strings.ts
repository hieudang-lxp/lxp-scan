// non-exported top-level util, copied verbatim into app-two — local utils
// get pasted around too, so unexported candidates are still fingerprinted
function collapseWhitespace(input: string) {
  return input.replace(/\s+/g, ' ').trim()
}

export const cleanLabel = (label: string) => collapseWhitespace(label)
