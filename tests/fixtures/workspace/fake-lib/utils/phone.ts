// same SHAPE as the email validators but a different regex literal —
// keeping literals verbatim is what stops these from clustering with email
export const isPhone = (phone: string) => {
  const re = /^\+?[0-9]{7,15}$/
  return re.test(String(phone).toLowerCase())
}

// identical body to isPhone in the SAME file — reported only with --same-file
export const checkPhone = (candidate: string) => {
  const re = /^\+?[0-9]{7,15}$/
  return re.test(String(candidate).toLowerCase())
}
