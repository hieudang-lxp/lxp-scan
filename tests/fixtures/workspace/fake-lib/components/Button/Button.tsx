export interface ButtonProps {
  variant?: 'primary' | 'ghost'
  size?: 'small' | 'large'
  disabled?: boolean
  onClick?: () => void
}

export const Button = ({ variant = 'primary', size, disabled, onClick }: ButtonProps) => {
  return null
}
