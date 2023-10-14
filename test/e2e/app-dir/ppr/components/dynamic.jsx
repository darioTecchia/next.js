import React from 'react'
import { cookies } from 'next/headers'
import { Login } from './login'

export function Dynamic({ fallback }) {
  const dynamic = fallback !== true

  let signedIn
  if (dynamic) {
    signedIn = cookies().has('session') ? true : false
  }

  if (!dynamic) {
    return (
      <div id="dynamic-fallback">
        <pre>Loading...</pre>
        <Login />
      </div>
    )
  }

  return (
    <div id="dynamic">
      <pre id="state" className={signedIn ? 'bg-green-600' : 'bg-red-600'}>
        {signedIn ? 'Signed In' : 'Not Signed In'}
      </pre>
      <Login signedIn={signedIn} />
    </div>
  )
}
