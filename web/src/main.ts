import { mount } from 'svelte'
import '@fontsource/barlow/400.css'
import '@fontsource/barlow/500.css'
import '@fontsource/barlow/600.css'
import '@fontsource/barlow-semi-condensed/500.css'
import '@fontsource/barlow-semi-condensed/600.css'
import './app.css'
import App from './App.svelte'

export default mount(App, { target: document.getElementById('app')! })
