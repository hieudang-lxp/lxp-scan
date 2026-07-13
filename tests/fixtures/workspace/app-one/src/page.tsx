import { Button } from 'fake-lib/components/Button'
import { formatThing } from 'utils/helpers'

export const Page = () => (
  <Button variant="primary" size="large" onClick={() => formatThing('x')}>
    {formatThing('ok')}
  </Button>
)
