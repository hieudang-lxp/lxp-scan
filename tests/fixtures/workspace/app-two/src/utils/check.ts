// same body as app-one's isEmail, declared as a function instead of an arrow
export function validateEmail(value: string) {
  const re = /^[^\s@]+@[^\s@]+\.[^\s@]+$/
  return re.test(String(value).toLowerCase())
}

// trivial passthrough — must be dropped by the min-token floor
export const alwaysTrue = () => true

// same body as app-one's isValidPhoneNumber, different name
export function checkPhoneNumber(raw: string) {
  const digits = String(raw).replace(/[^0-9+]/g, '')
  return digits.length >= 7 && digits.length <= 15
}
