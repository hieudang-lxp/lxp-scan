// The motivating clone case: identical body to app-two's validateEmail,
// different function and param names.
export const isEmail = (email: string) => {
  const re = /^[^\s@]+@[^\s@]+\.[^\s@]+$/
  return re.test(String(email).toLowerCase())
}

// substring of a clustered name — must never be linked to the isEmail cluster
export const isEmailFlow = (flow: string) => flow.startsWith('email:')

// trivial passthrough — must be dropped by the min-token floor
export const identity = (x: unknown) => !!x

// identical body to app-two's checkPhoneNumber, and the name already exists
// as an export of the npm-only lxp-common-functions-js package
export const isValidPhoneNumber = (input: string) => {
  const digits = String(input).replace(/[^0-9+]/g, '')
  return digits.length >= 7 && digits.length <= 15
}
