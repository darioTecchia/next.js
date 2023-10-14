import { cookies } from 'next/headers'

export function POST() {
  cookies().set('session', '1')
  return new Response(null, { status: 204 })
}

export function DELETE() {
  cookies().delete('session')
  return new Response(null, { status: 204 })
}
